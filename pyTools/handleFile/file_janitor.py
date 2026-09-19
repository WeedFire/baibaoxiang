# -*- coding: utf-8 -*-
"""
文件清理助手
==========================
根据 Excel 清单批量清理指定目录下的文件/文件夹，删除到系统回收站（可恢复）。

依赖：
    - PyQt5        现代化图形界面
    - openpyxl     读取 .xlsx（以及通过 xlrd 读取 .xls）
    - send2trash   安全删除到回收站（缺失时回退为普通删除并提示）

界面使用 PyQt5（现代风格，替代旧的 tkinter 界面）。
"""
import os
import sys
import json
import re
import time
import fnmatch
import traceback
import threading
from datetime import datetime

# xlrd 仅用于读取 .xls；缺失时仍可正常读取 .xlsx（方法内部会按需提示）
try:
    import xlrd
    XLRD_OK = True
except Exception:
    xlrd = None
    XLRD_OK = False

# ---------------- 第三方依赖（缺失时优雅降级） ----------------
try:
    import openpyxl
    OPENPYXL_OK = True
except Exception:
    openpyxl = None
    OPENPYXL_OK = False

try:
    from send2trash import send2trash
    SEND2TRASH_OK = True
except Exception:
    send2trash = None
    SEND2TRASH_OK = False

# ---------------- PyQt5 ----------------
try:
    from PyQt5.QtWidgets import (
        QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
        QLabel, QLineEdit, QPushButton, QComboBox, QRadioButton, QCheckBox,
        QSpinBox, QListWidget, QTableWidget, QTableWidgetItem, QHeaderView,
        QTextEdit, QProgressBar, QGroupBox, QScrollArea,
        QFileDialog, QMessageBox, QDialog, QDialogButtonBox, QMenu,
        QPlainTextEdit, QAbstractItemView, QFrame, QSizePolicy, QAction,
)
    from PyQt5.QtCore import Qt, QThread, pyqtSignal, QUrl
    from PyQt5.QtGui import QColor, QTextCharFormat, QTextCursor, QDesktopServices, QFont
except Exception as _qt_err:
    _msg = ("无法启动「文件清理助手」：缺少 PyQt5 图形界面库。\n\n"
            "请先安装依赖：\n    pip install PyQt5 openpyxl send2trash xlrd\n\n"
            "详细错误：%s" % _qt_err)
    sys.stderr.write(_msg + "\n")
    try:
        import ctypes
        if sys.platform == "win32":
            ctypes.windll.user32.MessageBoxW(0, _msg, "文件清理助手 - 启动失败", 0x10)
    except Exception:
        pass
    sys.exit(1)


# ==================== 常量与默认配置 ====================
APP_NAME = "文件清理助手"
APP_VERSION = "1.0.0"
CONFIG_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "config.json")

DELETE_TYPE_LABELS = {"file": "仅文件", "folder": "仅文件夹", "all": "文件和文件夹"}
MATCH_MODE_LABELS = {"exact": "精确匹配", "contains": "包含匹配", "regex": "正则匹配"}

# 系统保护目录前缀（Windows），不区分大小写
SYSTEM_PROTECTED = [
    r"C:\Windows",
    r"C:\Program Files",
    r"C:\Program Files (x86)",
    r"C:\ProgramData",
    os.path.join(os.environ.get("USERPROFILE", ""), "AppData"),
]
# 其它平台保护前缀
SYSTEM_PROTECTED_LINUX = ["/bin", "/boot", "/dev", "/etc", "/lib", "/proc",
                          "/root", "/sbin", "/sys", "/usr", "/var"]
SYSTEM_PROTECTED_MAC = ["/System", "/Library", "/Applications", "/private", "/usr"]

# 文件名非法字符（用于清洗过滤 Excel 条目）
ILLEGAL_CHARS = set('<>:"/\\|?*')


# ==================== 配置管理 ====================
def load_config():
    if os.path.isfile(CONFIG_FILE):
        try:
            with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {}


def save_config(cfg):
    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(cfg, f, ensure_ascii=False, indent=2)
    except Exception:
        pass


# ==================== 工具函数 ====================
def fmt_size(num_bytes):
    """字节 -> 人类可读大小"""
    if num_bytes is None:
        return "—"
    try:
        num_bytes = float(num_bytes)
    except Exception:
        return "—"
    if num_bytes < 0:
        return "—"
    units = ["B", "KB", "MB", "GB", "TB"]
    idx = 0
    while num_bytes >= 1024 and idx < len(units) - 1:
        num_bytes /= 1024.0
        idx += 1
    if idx == 0:
        return f"{int(num_bytes)} {units[idx]}"
    return f"{num_bytes:.2f} {units[idx]}"


def col_letter_to_index(letter):
    """'A'->0, 'B'->1 ..."""
    letter = (letter or "").strip().upper()
    n = 0
    for ch in letter:
        if not ("A" <= ch <= "Z"):
            return None
        n = n * 26 + (ord(ch) - ord("A") + 1)
    return n - 1 if n > 0 else None


def normalize_path(p):
    return os.path.normcase(os.path.abspath(p))


def to_native_path(p):
    """把路径归一化为「原生绝对路径」（去掉首尾引号/空白，统一分隔符）。

    背景：send2trash 在 Windows 上会自行给路径拼 ``\\\\?\\`` 长路径前缀，
    而该前缀只接受全反斜杠的本地绝对路径。Excel 清单里常见 ``D:/temp/x``
    这类正斜杠写法，拼出来就变成非法的 ``\\\\?\\D:/temp/x``，删除时报
    ``[Errno 3] 系统找不到指定的路径``。因此凡是要交给系统 API（删除、
    存在性检查、拼接）的路径，先经过本函数归一化。
    """
    p = (p or "").strip().strip('"').strip()
    if not p:
        return p
    try:
        p = os.path.abspath(p)
    except Exception:
        p = os.path.normpath(p)
    if os.name == "nt":
        p = p.replace("/", "\\")
    return p


def is_protected_path(path):
    """判断路径是否位于系统保护目录内"""
    ap = normalize_path(path)
    for pr in SYSTEM_PROTECTED + SYSTEM_PROTECTED_LINUX + SYSTEM_PROTECTED_MAC:
        if pr and ap.startswith(normalize_path(pr)):
            return True
    return False


def is_drive_root(path):
    """判断是否为磁盘根目录，如 C:\\ 或 C:"""
    p = path.rstrip("\\/").rstrip(":")
    norm = os.path.normpath(path)
    drive, tail = os.path.splitdrive(norm)
    if drive and (tail in ("", "\\", "/")):
        return True
    return False


# ==================== Excel 读取模块 ====================
class ExcelReader:
    """读取 Excel 指定工作表、指定列，做清洗/去空/去重。"""

    def __init__(self):
        self.file_path = None
        self.sheet_names = []
        self.headers = []
        self.selected_sheet = None
        self.selected_col_index = 0
        self.last_error = ""

    def open_file(self, file_path):
        self.file_path = file_path
        self.sheet_names = []
        self.headers = []
        self.last_error = ""
        ext = os.path.splitext(file_path)[1].lower()
        if ext == ".xlsx":
            if not OPENPYXL_OK:
                self.last_error = "未安装 openpyxl，无法读取 .xlsx"
                return False
            try:
                wb = openpyxl.load_workbook(file_path, read_only=True, data_only=True)
                self.sheet_names = wb.sheetnames
                wb.close()
            except PermissionError:
                self.last_error = "文件正被其他程序打开，请关闭后重试"
                return False
            except Exception as e:
                self.last_error = f"打开失败：{e}"
                return False
            return True
        elif ext == ".xls":
            try:
                import xlrd  # 可选
            except Exception:
                self.last_error = "未安装 xlrd，无法读取 .xls（请改用 .xlsx）"
                return False
            try:
                wb = xlrd.open_workbook(file_path)
                self.sheet_names = wb.sheet_names()
            except Exception as e:
                self.last_error = f"打开失败：{e}"
                return False
            return True
        else:
            self.last_error = "仅支持 .xlsx 和 .xls 格式"
            return False

    def load_sheet_headers(self, sheet_name):
        """读取指定工作表的表头行，返回列名列表（带列字母前缀）"""
        self.selected_sheet = sheet_name
        self.headers = []
        ext = os.path.splitext(self.file_path)[1].lower()
        rows = None
        try:
            if ext == ".xlsx":
                wb = openpyxl.load_workbook(self.file_path, read_only=True, data_only=True)
                ws = wb[sheet_name]
                it = ws.iter_rows(values_only=True)
                try:
                    first = next(it)
                except StopIteration:
                    first = None
                wb.close()
                if first:
                    rows = list(first)
            else:
                wb = xlrd.open_workbook(self.file_path)
                ws = wb.sheet_by_name(sheet_name)
                if ws.nrows > 0:
                    rows = ws.row_values(0)
        except Exception as e:
            self.last_error = f"读取表头失败：{e}"
            return []
        if not rows:
            return []
        headers = []
        for i, v in enumerate(rows):
            letter = ""
            n = i + 1
            while n > 0:
                n, r = divmod(n - 1, 26)
                letter = chr(65 + r) + letter
            name = "" if v is None else str(v).strip()
            headers.append(f"{letter}: {name}" if name else f"{letter}: (未命名)")
        self.headers = headers
        return headers

    def read_column(self, sheet_name, col_index, full_width=False):
        """
        读取指定列数据，返回 dict:
            valid: 去空、去重、清洗后的有效条目列表
            raw_count: 原始非空条目数
            empty_count: 空值数
            dup_count: 去重重数
            skipped_long: 超长条目数
            skipped_illegal: 含非法字符条目数
            preview: 前 20 行原始（清洗前）预览
        """
        result = {
            "valid": [], "raw_count": 0, "empty_count": 0, "dup_count": 0,
            "skipped_long": 0, "skipped_illegal": 0, "preview": [],
        }
        if col_index is None or col_index < 0:
            self.last_error = "列索引无效"
            return result
        ext = os.path.splitext(self.file_path)[1].lower()
        values = []
        try:
            if ext == ".xlsx":
                wb = openpyxl.load_workbook(self.file_path, read_only=True, data_only=True)
                ws = wb[sheet_name]
                rows = list(ws.iter_rows(values_only=True))
                wb.close()
                # 跳过表头行（第一行）
                for row in rows[1:]:
                    if col_index < len(row):
                        values.append(row[col_index])
                    else:
                        values.append(None)
            else:
                wb = xlrd.open_workbook(self.file_path)
                ws = wb.sheet_by_name(sheet_name)
                for r in range(1, ws.nrows):  # 跳过表头行
                    row = ws.row_values(r)
                    values.append(row[col_index] if col_index < len(row) else None)
        except Exception as e:
            self.last_error = f"读取列数据失败：{e}"
            return result

        seen = set()
        for v in values:
            if v is None:
                result["empty_count"] += 1
                continue
            s = str(v)
            if s.strip() == "":
                result["empty_count"] += 1
                continue
            result["raw_count"] += 1
            if full_width:
                s = self._to_half_width(s)
            s = s.strip()
            if len(result["preview"]) < 20:
                result["preview"].append(s)
            if len(s) > 260:
                result["skipped_long"] += 1
                continue
            if any(ch in ILLEGAL_CHARS for ch in s):
                result["skipped_illegal"] += 1
                continue
            if s in seen:
                result["dup_count"] += 1
                continue
            seen.add(s)
            result["valid"].append(s)
        return result

    @staticmethod
    def _to_half_width(text):
        """全角转半角（ASCII 区间内）"""
        out = []
        for ch in text:
            code = ord(ch)
            if code == 0x3000:
                out.append(" ")
            elif 0xFF01 <= code <= 0xFF5E:
                out.append(chr(code - 0xFEE0))
            else:
                out.append(ch)
        return "".join(out)


# ==================== 扫描引擎（后台线程） ====================
class ScanEngine:
    def __init__(self):
        self.stop_flag = False
        self.matched = []          # 结果 dict 列表
        self.matched_lock = threading.Lock()

    def stop(self):
        self.stop_flag = True

    def _match(self, name, matchers, mode, ignore_case):
        if mode == "exact":
            key = name.lower() if ignore_case else name
            return key in matchers["exact"]
        elif mode == "contains":
            low = name.lower() if ignore_case else name
            for sub in matchers["contains"]:
                if sub in low:
                    return True
            return False
        elif mode == "regex":
            for rx in matchers["regex"]:
                if rx.search(name):
                    return True
        return False

    def _build_matchers(self, names, mode, ignore_case):
        if mode == "exact":
            s = set()
            for n in names:
                s.add(n.lower() if ignore_case else n)
            return {"exact": s}
        elif mode == "contains":
            lst = [(n.lower() if ignore_case else n) for n in names]
            return {"contains": lst}
        elif mode == "regex":
            rxs = []
            for n in names:
                try:
                    rxs.append(re.compile(n, re.IGNORECASE) if ignore_case else re.compile(n))
                except re.error:
                    # 非法正则视为普通精确名匹配
                    rxs.append(re.compile(re.escape(n),
                                re.IGNORECASE) if ignore_case else re.compile(re.escape(n)))
            return {"regex": rxs}
        return {}

    def scan(self, names, target_paths, delete_type, match_mode, ignore_case,
             recursive, max_depth, exclude_dirs, whitelist,
             result_queue=None, result_cb=None, progress_cb=None, log_cb=None):
        """
        遍历目标路径，匹配名称，命中结果通过 result_cb(item) 回调推送（dict）。
        delete_type: file/folder/all
        match_mode: exact/contains/regex
        """
        self.matched = []
        self.stop_flag = False
        matchers = self._build_matchers(names, match_mode, ignore_case)
        exclude_set = set(d.lower() for d in exclude_dirs if d.strip())

        done = 0
        matched_count = 0
        scanned_count = 0

        for root in target_paths:
            if self.stop_flag:
                break
            if not os.path.isdir(root):
                if log_cb:
                    log_cb("WARN", f"路径不存在或不是目录，已跳过：{root}")
                continue
            if is_protected_path(root):
                if log_cb:
                    log_cb("WARN", f"系统保护目录，已跳过扫描：{root}")
                continue
            if is_drive_root(root):
                if log_cb:
                    log_cb("WARN", f"磁盘根目录扫描风险较高：{root}（已继续，请谨慎勾选）")

            for path, is_dir in self._walk(root, recursive, max_depth, exclude_set):
                if self.stop_flag:
                    break
                scanned_count += 1
                try:
                    if is_dir:
                        if delete_type == "file":
                            continue
                    else:
                        if delete_type == "folder":
                            continue
                    # 白名单
                    if self._in_whitelist(path, whitelist):
                        continue
                    # 系统保护（兜底）
                    if is_protected_path(path):
                        continue
                    name = os.path.basename(path)
                    if self._match(name, matchers, match_mode, ignore_case):
                        if os.path.islink(path):
                            continue  # 跳过符号链接，避免误删目标
                        try:
                            st = os.stat(path)
                            size = None if is_dir else st.st_size
                            mtime = datetime.fromtimestamp(st.st_mtime).strftime("%Y-%m-%d %H:%M")
                        except OSError:
                            size = None
                            mtime = "—"
                        item = {
                            "path": path,
                            "is_dir": is_dir,
                            "name": name,
                            "size": size,
                            "mtime": mtime,
                            "matched": name,
                        }
                        with self.matched_lock:
                            # 去重同一路径
                            if not any(m["path"] == path for m in self.matched):
                                self.matched.append(item)
                                matched_count += 1
                                if result_cb is not None:
                                    result_cb(item)
                                elif result_queue is not None:
                                    result_queue.put(item)
                except Exception:
                    continue
                done += 1
                if done % 200 == 0 and progress_cb:
                    progress_cb(done, matched_count)

        if progress_cb:
            progress_cb(done, matched_count)
        return scanned_count, matched_count

    @staticmethod
    def _in_whitelist(path, whitelist):
        """白名单匹配：命中即受保护，扫描与删除都不会动它。

        条目支持三种写法（大小写不敏感，Windows）：
          1) 目录（含路径分隔符或盘符，如 ``D:\\资料``）：保护该目录及其全部子项；
          2) 通配符（含 ``*`` 或 ``?``，如 ``*.pdf`` / ``报告*``）：按文件名匹配；
          3) 文件名（如 ``数据v1.pdf``，含后缀）：与文件名精确匹配。
        旧版只支持第 1 种，填文件名会静默失效（文件照删），故补上 2/3 两种。
        """
        if not whitelist:
            return False
        ap = normalize_path(path)              # 已 normcase（小写 + 反斜杠）
        base = os.path.basename(ap)
        for w in whitelist:
            w = (w or "").strip().strip('"').strip()
            if not w:
                continue
            # 通配符：按文件名匹配（fnmatch 在 Windows 上本身不区分大小写）
            if "*" in w or "?" in w:
                if fnmatch.fnmatch(base, w):
                    return True
                continue
            # 纯文件名（无分隔符、无盘符）：按文件名精确匹配
            is_dir_like = ("\\" in w or "/" in w
                           or (len(w) > 1 and w[1] == ":"))
            if not is_dir_like:
                if base == os.path.normcase(w):
                    return True
                continue
            # 目录/路径：前缀匹配，且要求目录边界（D:\资料 不保护 D:\资料2）
            wp = normalize_path(w)
            if ap == wp or ap.startswith(
                    wp if wp.endswith(os.sep) else wp + os.sep):
                return True
        return False

    def _walk(self, root, recursive, max_depth, exclude_set):
        """生成器：yield (path, is_dir)。支持深度限制与排除目录。"""
        root_depth = root.rstrip("\\/").count(os.sep)
        stack = [root]
        while stack:
            if self.stop_flag:
                return
            cur = stack.pop()
            try:
                entries = os.scandir(cur)
            except (PermissionError, OSError):
                continue
            dirs = []
            for e in entries:
                try:
                    is_dir = e.is_dir(follow_symlinks=False)
                except OSError:
                    is_dir = False
                if self.stop_flag:
                    return
                yield (e.path, is_dir)
                if is_dir:
                    dirs.append(e.path)
            if recursive:
                for d in dirs:
                    # 排除目录（按基线名或全路径匹配）
                    base = os.path.basename(d).lower()
                    if base in exclude_set:
                        continue
                    if max_depth is not None:
                        depth = d.rstrip("\\/").count(os.sep) - root_depth
                        if depth > max_depth:
                            continue
                    stack.append(d)


# ==================== 删除引擎（后台线程） ====================
class DeleteEngine:
    def __init__(self):
        self.stop_flag = False

    def stop(self):
        self.stop_flag = True

    def delete_items(self, items, whitelist, progress_cb, item_cb, log_cb):
        """
        items: list of dict(path, is_dir, size)
        通过 send2trash 删除，逐条回调 item_cb(dict) 与 progress_cb(done, total)
        """
        total = len(items)
        done = 0
        success = 0
        failed = 0
        skipped = 0
        freed = 0

        for it in items:
            if self.stop_flag:
                break
            # 归一化为原生反斜杠绝对路径：send2trash 会拼 ``\\?\`` 长路径前缀，
            # 正斜杠混用（如 D:/temp/x）会导致 Errno 3 找不到路径
            path = to_native_path(it["path"])
            is_dir = it["is_dir"]
            size = it.get("size") or 0
            item_type = "文件夹" if is_dir else "文件"
            try:
                # 白名单 / 系统保护兜底
                if ScanEngine._in_whitelist(path, whitelist):
                    if log_cb:
                        log_cb("WARN", f"白名单保护，跳过：{path}")
                    skipped += 1
                    if item_cb:
                        item_cb({"path": path, "type": item_type, "size": size,
                                 "result": "skipped", "error": "白名单保护"})
                    done += 1
                    if progress_cb:
                        progress_cb(done, total)
                    continue
                if is_protected_path(path):
                    if log_cb:
                        log_cb("WARN", f"系统保护，跳过：{path}")
                    skipped += 1
                    if item_cb:
                        item_cb({"path": path, "type": item_type, "size": size,
                                 "result": "skipped", "error": "系统保护目录"})
                    done += 1
                    if progress_cb:
                        progress_cb(done, total)
                    continue
                if not os.path.exists(path):
                    if log_cb:
                        log_cb("INFO", f"已不存在，跳过：{path}")
                    skipped += 1
                    if item_cb:
                        item_cb({"path": path, "type": item_type, "size": size,
                                 "result": "skipped", "error": "已不存在"})
                    done += 1
                    if progress_cb:
                        progress_cb(done, total)
                    continue

                if SEND2TRASH_OK:
                    send2trash(path)
                else:
                    # 回退：普通删除
                    if is_dir:
                        import shutil
                        shutil.rmtree(path)
                    else:
                        os.remove(path)
                success += 1
                freed += (size or 0)
                if log_cb:
                    log_cb("SUCCESS", f"已删除：{path}（{item_type}）")
                if item_cb:
                    item_cb({"path": path, "type": item_type, "size": size,
                             "result": "success", "error": ""})
            except Exception as e:
                failed += 1
                err = str(e)
                if "being used" in err or "拒绝访问" in err or "Permission" in err:
                    err = "文件被占用/权限不足"
                if log_cb:
                    log_cb("ERROR", f"删除失败：{path}，原因：{err}")
                if item_cb:
                    item_cb({"path": path, "type": item_type, "size": size,
                             "result": "failed", "error": err})
            done += 1
            if progress_cb:
                progress_cb(done, total)

        return {"total": total, "success": success, "failed": failed,
                "skipped": skipped, "freed": freed}


# ==================== 可排序表格项 ====================
class NumericItem(QTableWidgetItem):
    """用于按数值排序的列（如序号、大小）。"""

    def __init__(self, text, value):
        super().__init__(text)
        self._value = value

    def __lt__(self, other):
        try:
            return self._value < other._value
        except Exception:
            return str(self._value) < str(other._value)


# ==================== 后台工作线程 ====================
class ScanWorker(QThread):
    progress = pyqtSignal(int, int)
    log = pyqtSignal(str, str)
    result = pyqtSignal(object)
    finished = pyqtSignal(int, int)

    def __init__(self, engine, names, target_paths, delete_type, match_mode,
                 ignore_case, recursive, max_depth, exclude_dirs, whitelist):
        super().__init__()
        self.engine = engine
        self.names = names
        self.target_paths = target_paths
        self.delete_type = delete_type
        self.match_mode = match_mode
        self.ignore_case = ignore_case
        self.recursive = recursive
        self.max_depth = max_depth
        self.exclude_dirs = exclude_dirs
        self.whitelist = whitelist

    def run(self):
        try:
            scanned, matched = self.engine.scan(
                names=self.names,
                target_paths=self.target_paths,
                delete_type=self.delete_type,
                match_mode=self.match_mode,
                ignore_case=self.ignore_case,
                recursive=self.recursive,
                max_depth=self.max_depth if self.max_depth > 0 else None,
                exclude_dirs=self.exclude_dirs,
                whitelist=self.whitelist,
                result_cb=lambda item: self.result.emit(item),
                progress_cb=lambda d, m: self.progress.emit(d, m),
                log_cb=lambda lvl, msg: self.log.emit(lvl, msg),
            )
            self.finished.emit(scanned, matched)
        except Exception as e:
            self.log.emit("ERROR", "扫描线程异常：" +
                          "".join(traceback.format_exception_only(type(e), e)).strip())
            self.finished.emit(0, 0)


class DeleteWorker(QThread):
    progress = pyqtSignal(int, int)
    item = pyqtSignal(object)
    log = pyqtSignal(str, str)
    finished = pyqtSignal(object, float)

    def __init__(self, engine, items, whitelist):
        super().__init__()
        self.engine = engine
        self.items = items
        self.whitelist = whitelist

    def run(self):
        start = time.time()
        try:
            report = self.engine.delete_items(
                items=self.items,
                whitelist=self.whitelist,
                progress_cb=lambda d, t: self.progress.emit(d, t),
                item_cb=lambda r: self.item.emit(r),
                log_cb=lambda lvl, msg: self.log.emit(lvl, msg),
            )
        except Exception as e:
            self.log.emit("ERROR", "删除线程异常：" +
                          "".join(traceback.format_exception_only(type(e), e)).strip())
            report = {"success": 0, "failed": 0, "skipped": 0, "freed": 0}
        elapsed = time.time() - start
        self.finished.emit(report, elapsed)


# ==================== 主界面 ====================
class FileJanitorApp(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} v{APP_VERSION}")
        self.resize(1200, 800)
        self.setMinimumSize(1000, 700)

        self.cfg = load_config()
        self.excel = ExcelReader()
        self.excel_names = []          # 当前 Excel 有效条目
        self.excel_stats = {}
        self.target_paths = []         # 目标路径列表
        self.scan_engine = None
        self.scan_worker = None
        self.delete_engine = None
        self.delete_worker = None
        self.result_items = []         # 已加入表格的结果 dict
        self.checked = set()           # 已勾选的 path 集合（勾选真相来源）
        self.whitelist = self.cfg.get("whitelist_paths", [])
        self.delete_log = []           # 删除日志（导出用）
        self._loading = False          # 批量刷新表格时的保护标志

        # 当前匹配设置
        self.delete_type = self.cfg.get("delete_type", "all")
        self.match_mode = self.cfg.get("match_mode", "exact")
        self.ignore_case = self.cfg.get("ignore_case", True)
        self.recursive = self.cfg.get("recursive", True)
        self.max_depth = self.cfg.get("max_depth", 0)  # 0 = 无限制
        self.exclude_text = ",".join(self.cfg.get("exclude_paths", []))

        self._ready = False             # 构造完成前忽略设置变更信号
        self._build_ui()
        self._restore_config()
        self._apply_style()
        self._ready = True

    # ---------------- 样式 ----------------
    def _apply_style(self):
        self.setStyleSheet("""
            QMainWindow, QWidget {
                background: #eef3ee; color: #243029;
                font-family: "Microsoft YaHei UI", "Segoe UI", "PingFang SC",
                             "Microsoft YaHei", sans-serif;
            }

            /* 顶部品牌色带 */
            #appHeader {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                            stop:0 #2f9e6b, stop:1 #3aa0c2);
            }
            #appTitle {
                font-size: 19px; font-weight: 700; color: #ffffff; letter-spacing: 1px;
            }
            #topBtn {
                background: rgba(255,255,255,0.14); border: none; color: #ffffff;
                border-radius: 8px; padding: 6px 16px; font-size: 13px;
            }
            #topBtn:hover { background: rgba(255,255,255,0.28); }
            #topBtn:pressed { background: rgba(255,255,255,0.38); }

            /* 分组卡片 */
            QGroupBox {
                background: #ffffff; border: 1px solid #dde8df; border-radius: 12px;
                margin-top: 14px; padding: 16px 14px 12px 14px;
                font-weight: 600; color: #1f2e25; font-size: 13px;
            }
            QGroupBox::title {
                subcontrol-origin: margin; left: 14px; padding: 0 8px;
                color: #2f9e6b; background: #eef3ee;
            }

            QLabel { color: #4a5a50; font-size: 13px; }
            #infoLabel { color: #2f9e6b; font-weight: 600; }
            #successLabel { color: #1f9d55; font-weight: 600; font-size: 14px; }

            QLineEdit, QComboBox, QSpinBox, QListWidget, QPlainTextEdit {
                background: #ffffff; border: 1px solid #cdded0; border-radius: 8px;
                padding: 7px 10px; color: #243029; font-size: 13px;
                selection-background-color: #bfe3cf;
            }
            QLineEdit:hover, QComboBox:hover { border: 1px solid #a9c7b4; }
            QLineEdit:focus, QComboBox:focus, QSpinBox:focus { border: 1px solid #2f9e6b; }
            QListWidget { min-height: 60px; outline: 0; }
            QListWidget::item { padding: 4px; border-radius: 4px; }
            QListWidget::item:selected { background: #dcefee; color: #1f2e25; }

            QPushButton {
                background: #eaf1ec; border: 1px solid #cdded0; border-radius: 8px;
                padding: 8px 16px; color: #243029; font-size: 13px;
            }
            QPushButton:hover { background: #dde9e0; border-color: #b7cdbc; }
            QPushButton:pressed { background: #cfe0d3; }
            QPushButton:disabled { color: #9fb0a4; background: #f1f5f2; border-color: #e2eae4; }

            #accentBtn {
                background: #2f9e6b; color: #fff; border: none; font-weight: 600;
                padding: 10px 20px; font-size: 14px;
            }
            #accentBtn:hover { background: #29885d; }
            #accentBtn:pressed { background: #237a52; }
            #accentBtn:disabled { background: #a9c7b4; }

            #accentBtnSmall {
                background: #2f9e6b; color: #fff; border: none; border-radius: 8px;
                padding: 7px 14px; font-size: 13px;
            }
            #accentBtnSmall:hover { background: #29885d; }

            /* 破坏性操作：醒目的红色按钮 */
            #dangerBtn {
                background: #e5484d; color: #fff; border: none; font-weight: 600;
                padding: 10px 20px; font-size: 14px; border-radius: 8px;
            }
            #dangerBtn:hover { background: #d23b40; }
            #dangerBtn:pressed { background: #bd3439; }
            #dangerBtn:disabled { background: #e7a9ab; }

            #warnBtn {
                background: #ffe3e3; color: #c0392b; border: 1px solid #f3b6b6;
                border-radius: 8px; padding: 8px 16px;
            }
            #warnBtn:hover { background: #ffd0d0; }
            #warnBtn:disabled { color: #b9a3a3; background: #f3eeee; border-color: #ece2e2; }

            #ghostBtn {
                background: transparent; border: 1px solid #cdded0; border-radius: 8px;
                padding: 7px 14px; color: #3a4a40;
            }
            #ghostBtn:hover { background: #e7f0ea; border-color: #b7cdbc; }

            /* 结果表格 */
            #resultTable {
                background: #ffffff; border: 1px solid #dde8df; border-radius: 12px;
                gridline-color: #eef3ee; selection-background-color: #dcefee;
                font-size: 13px; alternate-background-color: #f6faf7;
            }
            #resultTable::item { padding: 6px 4px; }
            #resultTable::item:selected { color: #1f2e25; }
            #resultTable::item:hover { background: #eaf6ef; }
            QHeaderView::section {
                background: #e7f1ea; color: #1f2e25; border: none; padding: 9px 8px;
                font-weight: 600; font-size: 12px; border-bottom: 2px solid #d2e6d8;
            }
            QHeaderView::section:hover { background: #dcebdf; }

            QScrollArea { background: transparent; border: none; }

            QProgressBar {
                border: 1px solid #cdded0; border-radius: 8px; background: #eef3ee;
                text-align: center; height: 16px; color: #3a4a40; font-size: 12px;
            }
            QProgressBar::chunk {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                            stop:0 #2f9e6b, stop:1 #3aa0c2);
                border-radius: 7px;
            }

            QPlainTextEdit { font-family: Consolas, "Courier New", monospace; font-size: 12px; border-radius: 8px; }

            QComboBox QAbstractItemView {
                background: #fff; border: 1px solid #cdded0; border-radius: 8px;
                selection-background-color: #dcefee; outline: 0;
            }

            /* 现代细滚动条 */
            QScrollBar:vertical { background: #eef3ee; width: 10px; border-radius: 5px; }
            QScrollBar::handle:vertical { background: #c2d6c7; border-radius: 5px; min-height: 30px; }
            QScrollBar::handle:vertical:hover { background: #a9c4af; }
            QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0; }
            QScrollBar:horizontal { background: #eef3ee; height: 10px; border-radius: 5px; }
            QScrollBar::handle:horizontal { background: #c2d6c7; border-radius: 5px; min-width: 30px; }
            QScrollBar::handle:horizontal:hover { background: #a9c4af; }
            QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal { width: 0; }

            QCheckBox { spacing: 6px; color: #3a4a40; }
            QCheckBox::indicator { width: 16px; height: 16px; }
            QRadioButton { spacing: 6px; color: #3a4a40; }
            QDialog { background: #eef3ee; }
        """)

    # ---------------- UI 构建 ----------------
    def _build_ui(self):
        central = QWidget()
        self.setCentralWidget(central)
        root_layout = QVBoxLayout(central)
        root_layout.setContentsMargins(14, 14, 14, 14)
        root_layout.setSpacing(10)

        # 顶部标题栏（品牌色带）
        header = QWidget()
        header.setObjectName("appHeader")
        top = QHBoxLayout(header)
        top.setContentsMargins(18, 12, 18, 12)
        # title = QLabel(APP_NAME)
        # title.setObjectName("appTitle")
        # top.addWidget(title)
        top.addStretch(1)
        btn_help = QPushButton("帮助")
        btn_about = QPushButton("关于")
        btn_help.setObjectName("topBtn")
        btn_about.setObjectName("topBtn")
        btn_help.clicked.connect(self.show_help)
        btn_about.clicked.connect(self.show_about)
        top.addWidget(btn_help)
        top.addWidget(btn_about)
        root_layout.addWidget(header)

        # 主分割：左配置 / 右结果
        splitter = QHBoxLayout()
        splitter.setSpacing(10)
        left = self._build_left()
        right = self._build_right()
        splitter.addWidget(left, 0)
        splitter.addWidget(right, 1)
        root_layout.addLayout(splitter, 1)

        # 底部：日志 + 进度 + 删除
        self._build_bottom(root_layout)

    # 左侧滚动配置区
    def _build_left(self):
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll.setMinimumWidth(420)
        scroll.setMaximumWidth(460)
        container = QWidget()
        layout = QVBoxLayout(container)
        layout.setSpacing(12)
        layout.setContentsMargins(4, 4, 8, 4)

        # ① 数据源配置
        f1 = QGroupBox("① 数据源配置（Excel 清单）")
        f1_layout = QVBoxLayout(f1)
        f1_layout.setSpacing(6)
        f1_layout.addWidget(QLabel("Excel 文件："))

        ex_row = QHBoxLayout()
        self.excel_path_edit = QLineEdit()
        self.excel_path_edit.setPlaceholderText("选择 Excel 文件…")
        browse_btn = QPushButton("浏览")
        browse_btn.setObjectName("accentBtnSmall")
        browse_btn.clicked.connect(self.browse_excel)
        ex_row.addWidget(self.excel_path_edit, 1)
        ex_row.addWidget(browse_btn)
        f1_layout.addLayout(ex_row)

        sheet_row = QHBoxLayout()
        sheet_row.addWidget(QLabel("工作表："))
        self.sheet_combo = QComboBox()
        self.sheet_combo.setMinimumWidth(120)
        self.sheet_combo.currentTextChanged.connect(lambda _: self.on_sheet_changed())
        sheet_row.addWidget(self.sheet_combo, 1)
        sheet_row.addWidget(QLabel("数据列："))
        self.col_combo = QComboBox()
        self.col_combo.setMinimumWidth(120)
        self.col_combo.currentTextChanged.connect(lambda _: self.on_column_changed())
        sheet_row.addWidget(self.col_combo, 1)
        f1_layout.addLayout(sheet_row)

        self.excel_stat_label = QLabel("有效条目: 0 条")
        self.excel_stat_label.setObjectName("infoLabel")
        f1_layout.addWidget(self.excel_stat_label)

        preview_btn = QPushButton("预览前 20 行")
        preview_btn.setObjectName("ghostBtn")
        preview_btn.clicked.connect(self.preview_excel)
        f1_layout.addWidget(preview_btn, 0, Qt.AlignLeft)
        layout.addWidget(f1)

        # ② 扫描配置
        f2 = QGroupBox("② 扫描配置（目标路径）")
        f2_layout = QVBoxLayout(f2)
        f2_layout.setSpacing(6)
        f2_layout.addWidget(QLabel("目标路径："))

        self.path_list = QListWidget()
        self.path_list.setMaximumHeight(110)
        f2_layout.addWidget(self.path_list)

        pbtn = QHBoxLayout()
        add_btn = QPushButton("添加路径")
        del_btn = QPushButton("删除选中")
        clr_btn = QPushButton("清空")
        add_btn.setObjectName("ghostBtn")
        del_btn.setObjectName("ghostBtn")
        clr_btn.setObjectName("ghostBtn")
        add_btn.clicked.connect(self.add_target_path)
        del_btn.clicked.connect(self.remove_target_path)
        clr_btn.clicked.connect(self.clear_target_paths)
        pbtn.addWidget(add_btn)
        pbtn.addWidget(del_btn)
        pbtn.addWidget(clr_btn)
        pbtn.addStretch(1)
        f2_layout.addLayout(pbtn)

        f2_layout.addWidget(QLabel("排除目录（逗号分隔，如 .git,node_modules）："))
        self.exclude_edit = QLineEdit(self.exclude_text)
        self.exclude_edit.editingFinished.connect(self.on_settings_changed)
        f2_layout.addWidget(self.exclude_edit)

        wl_tip = QLabel(
            "白名单（永不被删，逗号分隔，支持三种写法）：\n"
            "    · 目录：D:\\资料       → 保护该目录及其全部子项\n"
            "    · 文件名：数据v1.pdf   → 精确保护同名文件（需带后缀）\n"
            "    · 通配符：*.pdf、报告* → 按文件名匹配（不分大小写）")
        wl_tip.setWordWrap(True)
        f2_layout.addWidget(wl_tip)
        self.whitelist_edit = QLineEdit(",".join(self.whitelist))
        self.whitelist_edit.setPlaceholderText(
            "例如：D:\\资料, 数据v1.pdf, *.pdf")
        self.whitelist_edit.editingFinished.connect(self.on_settings_changed)
        f2_layout.addWidget(self.whitelist_edit)
        layout.addWidget(f2)

        # ③ 匹配设置
        f3 = QGroupBox("③ 匹配设置")
        f3_layout = QVBoxLayout(f3)
        f3_layout.setSpacing(8)

        f3_layout.addWidget(QLabel("删除类型："))
        dt_row = QHBoxLayout()
        self.dt_group = self._radio_row(dt_row, [("file", "仅文件"), ("folder", "仅文件夹"), ("all", "全部")], self.delete_type)
        f3_layout.addLayout(dt_row)

        f3_layout.addWidget(QLabel("匹配模式："))
        mm_row = QHBoxLayout()
        self.mm_group = self._radio_row(mm_row, [("exact", "精确"), ("contains", "包含"), ("regex", "正则")], self.match_mode)
        f3_layout.addLayout(mm_row)

        opt_row = QHBoxLayout()
        self.rec_chk = QCheckBox("递归子目录")
        self.ic_chk = QCheckBox("忽略大小写")
        self.rec_chk.setChecked(self.recursive)
        self.ic_chk.setChecked(self.ignore_case)
        self.rec_chk.toggled.connect(self.on_settings_changed)
        self.ic_chk.toggled.connect(self.on_settings_changed)
        opt_row.addWidget(self.rec_chk)
        opt_row.addWidget(self.ic_chk)
        opt_row.addStretch(1)
        f3_layout.addLayout(opt_row)

        depth_row = QHBoxLayout()
        depth_row.addWidget(QLabel("最大深度（0 = 无限）："))
        self.depth_spin = QSpinBox()
        self.depth_spin.setRange(0, 20)
        self.depth_spin.setValue(self.max_depth)
        self.depth_spin.valueChanged.connect(self.on_settings_changed)
        depth_row.addWidget(self.depth_spin)
        depth_row.addStretch(1)
        f3_layout.addLayout(depth_row)
        layout.addWidget(f3)

        # 操作按钮
        act = QHBoxLayout()
        self.scan_btn = QPushButton("扫描预览")
        self.scan_btn.setObjectName("accentBtn")
        self.scan_stop_btn = QPushButton("取消扫描")
        self.scan_stop_btn.setObjectName("warnBtn")
        self.scan_stop_btn.setEnabled(False)
        self.clear_btn = QPushButton("清空结果")
        self.clear_btn.setObjectName("ghostBtn")
        self.scan_btn.clicked.connect(self.start_scan)
        self.scan_stop_btn.clicked.connect(self.stop_scan)
        self.clear_btn.clicked.connect(self.clear_results)
        act.addWidget(self.scan_btn)
        act.addWidget(self.scan_stop_btn)
        act.addWidget(self.clear_btn)
        act.addStretch(1)
        layout.addLayout(act)

        layout.addStretch(1)
        scroll.setWidget(container)
        return scroll

    def _radio_row(self, layout, options, current):
        group = []
        for val, lab in options:
            rb = QRadioButton(lab)
            rb.setProperty("value", val)
            # 先设初值再连接信号：避免 setChecked(True) 在构造期触发
            # on_settings_changed（此时 self.dt_group 尚未赋值 → AttributeError）
            if val == current:
                rb.setChecked(True)
            rb.toggled.connect(self.on_settings_changed)
            layout.addWidget(rb)
            group.append(rb)
        layout.addStretch(1)
        return group

    def _group_value(self, group):
        for rb in group:
            if rb.isChecked():
                return rb.property("value")
        return None

    # 右侧结果区
    def _build_right(self):
        widget = QWidget()
        layout = QVBoxLayout(widget)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)

        ctrl = QHBoxLayout()
        self.result_stat_label = QLabel("匹配：文件 0 | 文件夹 0 | 总大小 0")
        self.result_stat_label.setObjectName("successLabel")
        ctrl.addWidget(self.result_stat_label)

        ctrl.addStretch(1)
        ctrl.addWidget(QLabel("筛选："))
        self.filter_edit = QLineEdit()
        self.filter_edit.setPlaceholderText("按名称/路径过滤")
        self.filter_edit.setMaximumWidth(180)
        self.filter_edit.textChanged.connect(self.apply_filter)
        ctrl.addWidget(self.filter_edit)

        sel_all = QPushButton("全选")
        sel_none = QPushButton("全不选")
        sel_all.setObjectName("ghostBtn")
        sel_none.setObjectName("ghostBtn")
        sel_all.clicked.connect(self.select_all)
        sel_none.clicked.connect(self.select_none)
        ctrl.addWidget(sel_all)
        ctrl.addWidget(sel_none)
        layout.addLayout(ctrl)

        self.table = QTableWidget(0, 8)
        self.table.setObjectName("resultTable")
        headers = ["选择", "#", "类型", "名称", "完整路径", "大小", "修改时间", "匹配条目"]
        self.table.setHorizontalHeaderLabels(headers)
        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.table.setAlternatingRowColors(True)
        self.table.setSortingEnabled(True)
        self.table.verticalHeader().setVisible(False)
        header = self.table.horizontalHeader()
        header.setSectionResizeMode(0, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(1, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(2, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(3, QHeaderView.Stretch)
        header.setSectionResizeMode(4, QHeaderView.Stretch)
        header.setSectionResizeMode(5, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(6, QHeaderView.ResizeToContents)
        header.setSectionResizeMode(7, QHeaderView.ResizeToContents)
        self.table.itemChanged.connect(self.on_item_changed)
        self.table.setContextMenuPolicy(Qt.CustomContextMenu)
        self.table.customContextMenuRequested.connect(self.on_table_context_menu)
        layout.addWidget(self.table, 1)
        return widget

    # 底部日志 + 进度 + 删除
    def _build_bottom(self, parent_layout):
        bottom = QVBoxLayout()
        bottom.setSpacing(6)

        log_top = QHBoxLayout()
        log_top.addWidget(QLabel("④ 操作日志"))
        log_top.addStretch(1)
        clear_log = QPushButton("清空")
        export_log = QPushButton("导出日志")
        clear_log.setObjectName("ghostBtn")
        export_log.setObjectName("ghostBtn")
        clear_log.clicked.connect(lambda: self.log_view.clear())
        export_log.clicked.connect(self.export_log)
        log_top.addWidget(clear_log)
        log_top.addWidget(export_log)
        bottom.addLayout(log_top)

        self.log_view = QPlainTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.setMaximumHeight(130)
        bottom.addWidget(self.log_view)

        self.progress = QProgressBar()
        self.progress.setRange(0, 0)  # 默认无限（扫描时显示忙碌）
        bottom.addWidget(self.progress)

        del_row = QHBoxLayout()
        del_row.addStretch(1)
        self.del_btn = QPushButton("执行删除（移至回收站）")
        self.del_btn.setObjectName("dangerBtn")
        self.del_btn.setEnabled(False)
        self.del_btn.clicked.connect(self.start_delete)
        del_row.addWidget(self.del_btn)
        bottom.addLayout(del_row)

        parent_layout.addLayout(bottom)

    # ---------------- 配置恢复 ----------------
    def _restore_config(self):
        p = self.cfg.get("last_excel_path", "")
        if p and os.path.isfile(p):
            self.excel_path_edit.setText(p)
            self.load_excel_file(p, silent=True)
        for tp in self.cfg.get("last_target_paths", []):
            tp = to_native_path(tp)
            if tp and os.path.isdir(tp):
                self.target_paths.append(tp)
                self.path_list.addItem(tp)

    # ---------------- Excel 相关 ----------------
    def browse_excel(self):
        path, _ = QFileDialog.getOpenFileName(
            self, "选择 Excel 文件", "",
            "Excel files (*.xlsx *.xls);;All files (*.*)")
        if not path:
            return
        self.excel_path_edit.setText(path)
        self.load_excel_file(path)

    def load_excel_file(self, path, silent=False):
        if not OPENPYXL_OK and path.lower().endswith(".xlsx"):
            QMessageBox.critical(self, "依赖缺失",
                                 "未安装 openpyxl，无法读取 .xlsx 文件。\n请执行：pip install openpyxl")
            return
        if not self.excel.open_file(path):
            if not silent:
                QMessageBox.critical(self, "错误", self.excel.last_error)
            return
        self.sheet_combo.blockSignals(True)
        self.sheet_combo.clear()
        self.sheet_combo.addItems(self.excel.sheet_names)
        self.sheet_combo.blockSignals(False)
        if self.excel.sheet_names:
            self.sheet_combo.setCurrentIndex(0)
            self.on_sheet_changed()
        self.persist_config()

    def on_sheet_changed(self):
        name = self.sheet_combo.currentText()
        if not name:
            return
        headers = self.excel.load_sheet_headers(name)
        self.col_combo.blockSignals(True)
        self.col_combo.clear()
        self.col_combo.addItems(headers)
        self.col_combo.blockSignals(False)
        if headers:
            idx = self.cfg.get("last_column_index", 0)
            if idx < len(headers):
                self.col_combo.setCurrentIndex(idx)
            else:
                self.col_combo.setCurrentIndex(0)
            self.on_column_changed()

    def on_column_changed(self):
        sel = self.col_combo.currentText()
        if not sel:
            return
        letter = sel.split(":")[0].strip()
        idx = col_letter_to_index(letter)
        self.excel.selected_col_index = idx
        self.read_column_data()

    def read_column_data(self):
        sheet = self.sheet_combo.currentText()
        idx = self.excel.selected_col_index
        if not sheet or idx is None:
            return
        stats = self.excel.read_column(sheet, idx)
        self.excel_stats = stats
        self.excel_names = stats["valid"]
        total = len(self.excel_names)
        self.excel_stat_label.setText(
            f"有效条目: {total} 条（原始 {stats['raw_count']}，空 {stats['empty_count']}，"
            f"重复 {stats['dup_count']}，超长/非法 {stats['skipped_long'] + stats['skipped_illegal']}）")
        if total == 0 and self.excel.last_error:
            QMessageBox.warning(self, "提示", self.excel.last_error)
        self.persist_config()

    def preview_excel(self):
        if not self.excel_stats.get("preview"):
            QMessageBox.information(self, "预览", "请先选择 Excel 并读取列数据。")
            return
        dlg = QDialog(self)
        dlg.setWindowTitle("Excel 前 20 行预览")
        dlg.resize(420, 400)
        v = QVBoxLayout(dlg)
        tv = QTableWidget(len(self.excel_stats["preview"]), 2)
        tv.setHorizontalHeaderLabels(["#", "内容"])
        tv.verticalHeader().setVisible(False)
        tv.setEditTriggers(QAbstractItemView.NoEditTriggers)
        tv.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeToContents)
        tv.horizontalHeader().setSectionResizeMode(1, QHeaderView.Stretch)
        for i, val in enumerate(self.excel_stats["preview"], 1):
            tv.setItem(i - 1, 0, QTableWidgetItem(str(i)))
            tv.setItem(i - 1, 1, QTableWidgetItem(val))
        v.addWidget(tv)
        btn = QPushButton("关闭")
        btn.setObjectName("ghostBtn")
        btn.clicked.connect(dlg.accept)
        v.addWidget(btn, 0, Qt.AlignRight)
        dlg.exec_()

    # ---------------- 目标路径 ----------------
    def add_target_path(self):
        path = QFileDialog.getExistingDirectory(self, "选择目标目录（可重复添加多个）")
        if not path:
            return
        path = to_native_path(path)
        if is_drive_root(path):
            ans = QMessageBox.question(
                self, "高风险警告",
                f"您选择的是磁盘根目录：{path}\n误删可能导致系统/数据严重损失！\n确认继续？",
                QMessageBox.Yes | QMessageBox.No)
            if ans != QMessageBox.Yes:
                return
        if path not in self.target_paths:
            self.target_paths.append(path)
            self.path_list.addItem(path)
            self.persist_config()

    def remove_target_path(self):
        item = self.path_list.currentItem()
        if not item:
            return
        path = item.text()
        if path in self.target_paths:
            self.target_paths.remove(path)
        self.path_list.takeItem(self.path_list.row(item))

    def clear_target_paths(self):
        self.target_paths.clear()
        self.path_list.clear()

    # ---------------- 设置变更 ----------------
    def on_settings_changed(self):
        if not getattr(self, "_ready", False):
            return  # 构造期内控件设初值触发的信号，直接忽略
        self.delete_type = self._group_value(self.dt_group) or "all"
        self.match_mode = self._group_value(self.mm_group) or "exact"
        self.ignore_case = self.ic_chk.isChecked()
        self.recursive = self.rec_chk.isChecked()
        self.max_depth = self.depth_spin.value()
        self.persist_config()

    def persist_config(self):
        cfg = {
            "last_excel_path": self.excel_path_edit.text(),
            "last_sheet_name": self.sheet_combo.currentText(),
            "last_column_index": self.excel.selected_col_index,
            "last_target_paths": self.target_paths,
            "delete_type": self._group_value(self.dt_group) or self.delete_type,
            "match_mode": self._group_value(self.mm_group) or self.match_mode,
            "recursive": self.rec_chk.isChecked(),
            "max_depth": self.depth_spin.value(),
            "ignore_case": self.ic_chk.isChecked(),
            "exclude_paths": [x.strip() for x in self.exclude_edit.text().split(",") if x.strip()],
            "whitelist_paths": [x.strip() for x in self.whitelist_edit.text().split(",") if x.strip()],
        }
        save_config(cfg)

    # ---------------- 扫描 ----------------
    def start_scan(self):
        if not self.excel_names:
            QMessageBox.warning(self, "提示", "Excel 有效条目为 0，无法扫描（请检查数据源）。")
            return
        if not self.target_paths:
            QMessageBox.warning(self, "提示", "请先添加至少一个目标路径。")
            return
        self.on_settings_changed()
        # 统一目标路径写法（正/反斜杠混用会让 send2trash 的 \\?\ 前缀失效）
        self.target_paths = [to_native_path(p) for p in self.target_paths]
        self.whitelist = [x.strip() for x in self.whitelist_edit.text().split(",") if x.strip()]
        exclude = [x.strip() for x in self.exclude_edit.text().split(",") if x.strip()]

        self.clear_results()
        self.scan_engine = ScanEngine()
        self.scan_btn.setEnabled(False)
        self.scan_stop_btn.setEnabled(True)
        self.del_btn.setEnabled(False)
        self.progress.setRange(0, 0)  # 忙碌模式
        self.log("INFO", f"开始扫描，目标路径 {len(self.target_paths)} 个，模式 "
                         f"{MATCH_MODE_LABELS.get(self.match_mode)}/{DELETE_TYPE_LABELS.get(self.delete_type)}")
        if self.whitelist:
            self.log("INFO", f"白名单保护 {len(self.whitelist)} 条："
                             + "、".join(self.whitelist))

        self.scan_worker = ScanWorker(
            self.scan_engine, self.excel_names, self.target_paths,
            self.delete_type, self.match_mode, self.ignore_case,
            self.recursive, self.max_depth, exclude, self.whitelist)
        self.scan_worker.result.connect(self.on_result)
        self.scan_worker.log.connect(lambda lvl, msg: self.log(lvl, msg))
        self.scan_worker.finished.connect(self.on_scan_finished)
        self.scan_worker.start()

    def on_result(self, item):
        self.result_items.append(item)
        self._insert_row(item)

    def on_scan_progress(self, done, matched):
        # 进度无法预知总数，保持忙碌模式即可
        pass

    def stop_scan(self):
        if self.scan_engine:
            self.scan_engine.stop()
        self.log("INFO", "已请求取消扫描…")

    def on_scan_finished(self, scanned, matched):
        self.scan_btn.setEnabled(True)
        self.scan_stop_btn.setEnabled(False)
        self.progress.setRange(0, 100)
        self.progress.setValue(0)
        if self.scan_engine and self.scan_engine.stop_flag:
            self.log("INFO", f"扫描已取消。已匹配 {len(self.result_items)} 项。")
        else:
            self.log("INFO", f"扫描完成，扫描 {scanned} 项，匹配 {matched} 项。")
        self.update_result_stats()
        if self.result_items:
            self.del_btn.setEnabled(True)

    # ---------------- 结果表格操作 ----------------
    def _insert_row(self, it):
        self._loading = True
        # 排序开启时逐行 setItem 会触发实时重排，导致单元格错位/空行，
        # 填充期间必须临时关闭排序（_rebuild_table 同理）
        self.table.setSortingEnabled(False)
        row = self.table.rowCount()
        self.table.insertRow(row)
        cb = QTableWidgetItem()
        cb.setFlags(Qt.ItemIsEnabled | Qt.ItemIsUserCheckable)
        cb.setCheckState(Qt.Checked)
        cb.setData(Qt.UserRole, it)
        self.table.setItem(row, 0, cb)
        self.checked.add(it["path"])
        self.table.setItem(row, 1, NumericItem(str(len(self.result_items)), len(self.result_items)))
        self.table.setItem(row, 2, QTableWidgetItem("文件夹" if it["is_dir"] else "文件"))
        self.table.setItem(row, 3, QTableWidgetItem(it["name"]))
        self.table.setItem(row, 4, QTableWidgetItem(it["path"]))
        self.table.setItem(row, 5, NumericItem(fmt_size(it["size"]), it["size"] or -1))
        self.table.setItem(row, 6, QTableWidgetItem(it["mtime"]))
        self.table.setItem(row, 7, QTableWidgetItem(it["matched"]))
        self.table.setSortingEnabled(True)
        self._loading = False
        self.update_result_stats()

    def _rebuild_table(self):
        """依据 self.result_items + self.checked + 筛选关键字重建表格"""
        kw = self.filter_edit.text().strip().lower()
        self._loading = True
        self.table.setSortingEnabled(False)
        self.table.setRowCount(0)
        for idx, it in enumerate(self.result_items, 1):
            if kw:
                text = " ".join([it["name"], it["path"], it["matched"]]).lower()
                if kw not in text:
                    continue
            row = self.table.rowCount()
            self.table.insertRow(row)
            cb = QTableWidgetItem()
            cb.setFlags(Qt.ItemIsEnabled | Qt.ItemIsUserCheckable)
            cb.setCheckState(Qt.Checked if it["path"] in self.checked else Qt.Unchecked)
            cb.setData(Qt.UserRole, it)
            self.table.setItem(row, 0, cb)
            self.table.setItem(row, 1, NumericItem(str(idx), idx))
            self.table.setItem(row, 2, QTableWidgetItem("文件夹" if it["is_dir"] else "文件"))
            self.table.setItem(row, 3, QTableWidgetItem(it["name"]))
            self.table.setItem(row, 4, QTableWidgetItem(it["path"]))
            self.table.setItem(row, 5, NumericItem(fmt_size(it["size"]), it["size"] or -1))
            self.table.setItem(row, 6, QTableWidgetItem(it["mtime"]))
            self.table.setItem(row, 7, QTableWidgetItem(it["matched"]))
        self.table.setSortingEnabled(True)
        self._loading = False

    def update_result_stats(self):
        files = sum(1 for it in self.result_items if not it["is_dir"])
        folders = sum(1 for it in self.result_items if it["is_dir"])
        total = sum((it["size"] or 0) for it in self.result_items if it["size"])
        self.result_stat_label.setText(
            f"匹配：文件 {files} | 文件夹 {folders} | 总大小 {fmt_size(total)}")

    def select_all(self):
        for it in self.result_items:
            self.checked.add(it["path"])
        self._rebuild_table()

    def select_none(self):
        self.checked.clear()
        self._rebuild_table()

    def on_item_changed(self, item):
        if self._loading:
            return
        if item.column() != 0:
            return
        it = item.data(Qt.UserRole)
        if it is None:
            return
        if item.checkState() == Qt.Checked:
            self.checked.add(it["path"])
        else:
            self.checked.discard(it["path"])

    def on_table_context_menu(self, pos):
        row = self.table.indexAt(pos).row()
        if row < 0:
            return
        menu = QMenu(self)
        act_open = QAction("打开所在文件夹", self)
        act_copy = QAction("复制路径", self)
        act_excl = QAction("排除此项", self)
        act_open.triggered.connect(self.ctx_open_folder)
        act_copy.triggered.connect(self.ctx_copy_path)
        act_excl.triggered.connect(self.ctx_exclude)
        menu.addAction(act_open)
        menu.addAction(act_copy)
        menu.addAction(act_excl)
        menu.exec_(self.table.viewport().mapToGlobal(pos))

    def ctx_open_folder(self):
        row = self.table.currentRow()
        if row < 0:
            return
        path = self.table.item(row, 4).text()
        folder = os.path.dirname(path)
        QDesktopServices.openUrl(QUrl.fromLocalFile(folder))

    def ctx_copy_path(self):
        row = self.table.currentRow()
        if row < 0:
            return
        path = self.table.item(row, 4).text()
        QApplication.clipboard().setText(path)

    def ctx_exclude(self):
        row = self.table.currentRow()
        if row < 0:
            return
        path = self.table.item(row, 4).text()
        folder = os.path.dirname(path)
        if folder not in self.whitelist:
            self.whitelist.append(folder)
            self.whitelist = list(dict.fromkeys(self.whitelist))
            self.whitelist_edit.setText(",".join(self.whitelist))
            self.persist_config()
        self.result_items = [it for it in self.result_items if it["path"] != path]
        self.checked.discard(path)
        self._rebuild_table()
        self.update_result_stats()
        self.log("INFO", f"已排除并加入白名单：{folder}")

    def apply_filter(self, *_):
        self._rebuild_table()

    def clear_results(self):
        self.table.setRowCount(0)
        self.result_items = []
        self.checked.clear()
        self.update_result_stats()

    # ---------------- 删除执行 ----------------
    def get_selected_items(self):
        return [it for it in self.result_items if it["path"] in self.checked]

    def start_delete(self):
        items = self.get_selected_items()
        if not items:
            QMessageBox.information(self, "提示", "请先勾选要删除的项。")
            return
        files = sum(1 for it in items if not it["is_dir"])
        folders = sum(1 for it in items if it["is_dir"])
        total = sum((it["size"] or 0) for it in items if it["size"])
        if not self.show_confirm_dialog(files, folders, total):
            return

        self.delete_log = []
        self.delete_engine = DeleteEngine()
        self.del_btn.setEnabled(False)
        self.progress.setRange(0, 100)
        self.progress.setValue(0)
        self.log("INFO", f"开始执行删除，共 {len(items)} 项（移至回收站）。")

        self.delete_worker = DeleteWorker(self.delete_engine, items, self.whitelist)
        self.delete_worker.progress.connect(self.on_delete_progress)
        self.delete_worker.item.connect(self.on_delete_item)
        self.delete_worker.log.connect(lambda lvl, msg: self.log(lvl, msg))
        self.delete_worker.finished.connect(self.on_delete_finished)
        self.delete_worker.start()

    def on_delete_progress(self, done, total):
        if total > 0:
            self.progress.setValue(int(done / total * 100))

    def on_delete_item(self, rec):
        self.delete_log.append(rec)
        self.result_items = [it for it in self.result_items if it["path"] != rec["path"]]
        self.checked.discard(rec["path"])
        self._rebuild_table()
        self.update_result_stats()

    def on_delete_finished(self, report, elapsed):
        self.del_btn.setEnabled(bool(self.result_items))
        self.progress.setValue(100)
        self.log("INFO", f"删除完成：成功 {report['success']}，失败 {report['failed']}，"
                         f"跳过 {report['skipped']}，释放 {fmt_size(report['freed'])}，"
                         f"耗时 {int(elapsed)} 秒。")
        self.show_report_dialog(report, elapsed)

    # ---------------- 对话框 ----------------
    def show_confirm_dialog(self, files, folders, total):
        dlg = QDialog(self)
        dlg.setWindowTitle("确认删除")
        dlg.setModal(True)
        v = QVBoxLayout(dlg)
        v.setSpacing(10)
        v.addWidget(QLabel("即将把以下内容删除到回收站（可恢复）："))
        info = QLabel(f"· 文件：{files} 个\n· 文件夹：{folders} 个\n· 总大小：{fmt_size(total)}")
        v.addWidget(info)

        ack = QCheckBox("我已确认以上内容无误")
        v.addWidget(ack)

        btns = QDialogButtonBox(QDialogButtonBox.Cancel | QDialogButtonBox.Ok)
        ok_btn = btns.button(QDialogButtonBox.Ok)
        ok_btn.setText("确认删除")
        ok_btn.setEnabled(False)
        btns.button(QDialogButtonBox.Cancel).setText("取消")
        ack.stateChanged.connect(
            lambda s: ok_btn.setEnabled(s == Qt.Checked))
        btns.accepted.connect(dlg.accept)
        btns.rejected.connect(dlg.reject)
        v.addWidget(btns)
        return dlg.exec_() == QDialog.Accepted

    def show_report_dialog(self, report, elapsed):
        dlg = QDialog(self)
        dlg.setWindowTitle("删除完成")
        dlg.setModal(True)
        v = QVBoxLayout(dlg)
        v.setSpacing(10)
        title = QLabel("✓ 删除完成")
        title.setStyleSheet("font-size:14px; font-weight:700; color:#1f9d55;")
        v.addWidget(title)
        text = (f"成功删除：{report['success']} 项\n"
                f"删除失败：{report['failed']} 项\n"
                f"跳过：{report['skipped']} 项\n"
                f"释放空间：{fmt_size(report['freed'])}\n"
                f"耗时：{int(elapsed)} 秒")
        v.addWidget(QLabel(text))

        fails = [r for r in self.delete_log if r["result"] == "failed"]
        if fails:
            fl = QLabel("失败详情：")
            fl.setStyleSheet("color:#c0392b;")
            v.addWidget(fl)
            lb = QListWidget()
            lb.setMaximumHeight(120)
            for r in fails:
                lb.addItem(f"· {r['path']}（{r['error']}）")
            v.addWidget(lb)

        note = QLabel("所有删除的文件可在系统回收站中恢复。")
        note.setStyleSheet("color:#888;")
        v.addWidget(note)

        btns = QHBoxLayout()
        btns.addStretch(1)
        export_btn = QPushButton("导出日志")
        open_btn = QPushButton("打开回收站")
        close_btn = QPushButton("关闭")
        export_btn.setObjectName("ghostBtn")
        open_btn.setObjectName("ghostBtn")
        close_btn.setObjectName("accentBtnSmall")
        export_btn.clicked.connect(self.export_log)
        open_btn.clicked.connect(self.open_recycle_bin)
        close_btn.clicked.connect(dlg.accept)
        btns.addWidget(export_btn)
        btns.addWidget(open_btn)
        btns.addWidget(close_btn)
        v.addLayout(btns)
        dlg.exec_()

    def open_recycle_bin(self):
        QDesktopServices.openUrl(QUrl("shell:RecycleBinFolder"))

    # ---------------- 日志 ----------------
    def log(self, level, msg):
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        color = {"INFO": "#41504a", "WARN": "#d97706",
                 "SUCCESS": "#1f9d55", "ERROR": "#dc2626"}.get(level, "#41504a")
        cursor = self.log_view.textCursor()
        cursor.movePosition(QTextCursor.End)
        fmt = QTextCharFormat()
        fmt.setForeground(QColor(color))
        cursor.setCharFormat(fmt)
        cursor.insertText(f"[{ts}] [{level}] {msg}\n")
        self.log_view.setTextCursor(cursor)

    def export_log(self):
        if not self.delete_log:
            QMessageBox.information(self, "提示", "暂无删除日志可导出。")
            return
        path, sel = QFileDialog.getSaveFileName(
            self, "导出日志", "delete_log.txt",
            "文本文件 (*.txt);;Excel (*.xlsx)")
        if not path:
            return
        if path.lower().endswith(".xlsx"):
            if not OPENPYXL_OK:
                QMessageBox.critical(self, "错误", "需要 openpyxl 才能导出 xlsx")
                return
            wb = openpyxl.Workbook()
            ws = wb.active
            ws.title = "删除日志"
            ws.append(["序号", "时间", "类型", "路径", "大小", "结果", "错误原因"])
            for i, r in enumerate(self.delete_log, 1):
                ws.append([i, datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
                           r["type"], r["path"], fmt_size(r["size"]),
                           r["result"], r["error"]])
            wb.save(path)
        else:
            with open(path, "w", encoding="utf-8") as f:
                f.write("文件清理助手 删除日志\n")
                f.write(f"导出时间：{datetime.now()}\n\n")
                for i, r in enumerate(self.delete_log, 1):
                    f.write(f"{i}\t{r['type']}\t{r['path']}\t{fmt_size(r['size'])}\t"
                            f"{r['result']}\t{r['error']}\n")
        QMessageBox.information(self, "完成", f"日志已导出：{path}")

    # ---------------- 帮助/关于 ----------------
    def show_help(self):
        txt = (
            "使用步骤：\n"
            "1. 选择 Excel 清单，指定工作表与数据列（默认第一列）。\n"
            "2. 添加目标扫描路径，可设置排除目录与白名单。\n"
            "3. 选择删除类型/匹配模式，点击「扫描预览」。\n"
            "4. 在右侧结果中勾选要删除的项（默认全选）。\n"
            "5. 点击「执行删除」，确认后移至回收站（可恢复）。\n\n"
            "白名单写法（逗号分隔，命中即永不删除）：\n"
            "   · 目录：D:\\资料（保护该目录及其子项）\n"
            "   · 文件名：数据v1.pdf（需带后缀，按文件名精确匹配）\n"
            "   · 通配符：*.pdf、报告*（按文件名匹配，不分大小写）\n\n"
            "安全机制：系统目录保护、白名单、二次确认、回收站恢复、全量日志。"
        )
        QMessageBox.information(self, "帮助", txt)

    def show_about(self):
        QMessageBox.information(self, "关于",
            f"{APP_NAME}\n版本 {APP_VERSION}\n\n"
            "Excel 清单驱动的批量文件清理工具。\n"
            "删除到回收站，安全可恢复。\n\n"
            "百宝箱 (Toolbox) 组件")


# ==================== 程序入口 ====================
def _report_error(text):
    """把运行错误同时写日志并弹窗，避免『直接失败』却看不到原因。"""
    try:
        _log = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                            "file_janitor_error.log")
        with open(_log, "a", encoding="utf-8") as f:
            f.write("==== %s ====\n%s\n" % (datetime.now().isoformat(), text))
    except Exception:
        pass
    try:
        import ctypes
        if sys.platform == "win32":
            ctypes.windll.user32.MessageBoxW(0, text,
                                             "文件清理助手 - 运行错误", 0x10)
        else:
            print(text)
    except Exception:
        print(text)


def main():
    """百宝箱入口函数"""
    if not OPENPYXL_OK:
        print("警告: openpyxl 未安装，无法读取 .xlsx（pip install openpyxl）")
    if not SEND2TRASH_OK:
        print("警告: send2trash 未安装，删除将不可恢复（pip install send2trash）")
    if not XLRD_OK:
        print("提示: xlrd 未安装，无法读取 .xls（.xlsx 不受影响）")

    # 事件循环内的未捕获异常也写入日志并弹窗
    sys.excepthook = lambda et, ex, tb: _report_error(
        "".join(traceback.format_exception(et, ex, tb)))

    try:
        app = QApplication.instance() or QApplication(sys.argv)
        app.setStyle("Fusion")
        app.setAttribute(Qt.AA_EnableHighDpiScaling, True)
        app.setAttribute(Qt.AA_UseHighDpiPixmaps, True)
        window = FileJanitorApp()
        window.show()
        sys.exit(app.exec_())
    except Exception:
        _report_error("".join(traceback.format_exception(*sys.exc_info())))
        sys.exit(1)


if __name__ == "__main__":
    main()
