# -*- coding: utf-8 -*-
"""
文件清理助手 (FileJanitor)
==========================
根据 Excel 清单批量清理指定目录下的文件/文件夹，删除到系统回收站（可恢复）。

依赖：
    - openpyxl  读取 .xlsx（以及通过 xlrd 读取 .xls）
    - send2trash  安全删除到回收站（缺失时回退为普通删除并提示）

界面使用 tkinter（与百宝箱其它小工具风格一致）。
"""
import os
import sys
import json
import re
import time
import threading
import tkinter as tk
from tkinter import ttk, filedialog, messagebox, scrolledtext
from datetime import datetime
from queue import Queue, Empty
import xlrd

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


# ==================== 常量与默认配置 ====================
APP_NAME = "文件清理助手 (FileJanitor)"
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

# 勾选/未勾选显示字符
CHECKED = "☑"
UNCHECKED = "☐"


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
    # C:\\ -> normpath = C:\\
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
             recursive, max_depth, exclude_dirs, whitelist, result_queue,
             progress_cb, log_cb):
        """
        遍历目标路径，匹配名称，命中结果通过 result_queue 推送（dict）。
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
                log_cb("WARN", f"路径不存在或不是目录，已跳过：{root}")
                continue
            if is_protected_path(root):
                log_cb("WARN", f"系统保护目录，已跳过扫描：{root}")
                continue
            if is_drive_root(root):
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
                                result_queue.put(item)
                except Exception:
                    continue
                done += 1
                if done % 200 == 0:
                    progress_cb(done, matched_count)

        progress_cb(done, matched_count)
        return scanned_count, matched_count

    @staticmethod
    def _in_whitelist(path, whitelist):
        if not whitelist:
            return False
        ap = normalize_path(path)
        for w in whitelist:
            if w and ap.startswith(normalize_path(w)):
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
            path = it["path"]
            is_dir = it["is_dir"]
            size = it.get("size") or 0
            item_type = "文件夹" if is_dir else "文件"
            try:
                # 白名单 / 系统保护兜底
                if ScanEngine._in_whitelist(path, whitelist):
                    log_cb("WARN", f"白名单保护，跳过：{path}")
                    skipped += 1
                    item_cb({"path": path, "type": item_type, "size": size,
                             "result": "skipped", "error": "白名单保护"})
                    done += 1
                    progress_cb(done, total)
                    continue
                if is_protected_path(path):
                    log_cb("WARN", f"系统保护，跳过：{path}")
                    skipped += 1
                    item_cb({"path": path, "type": item_type, "size": size,
                             "result": "skipped", "error": "系统保护目录"})
                    done += 1
                    progress_cb(done, total)
                    continue
                if not os.path.exists(path):
                    log_cb("INFO", f"已不存在，跳过：{path}")
                    skipped += 1
                    item_cb({"path": path, "type": item_type, "size": size,
                             "result": "skipped", "error": "已不存在"})
                    done += 1
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
                log_cb("SUCCESS", f"已删除：{path}（{item_type}）")
                item_cb({"path": path, "type": item_type, "size": size,
                         "result": "success", "error": ""})
            except Exception as e:
                failed += 1
                err = str(e)
                if "being used" in err or "拒绝访问" in err or "Permission" in err:
                    err = "文件被占用/权限不足"
                log_cb("ERROR", f"删除失败：{path}，原因：{err}")
                item_cb({"path": path, "type": item_type, "size": size,
                         "result": "failed", "error": err})
            done += 1
            progress_cb(done, total)

        return {"total": total, "success": success, "failed": failed,
                "skipped": skipped, "freed": freed}


# ==================== 主界面 ====================
class FileJanitorApp:
    def __init__(self, root):
        self.root = root
        self.root.title(f"{APP_NAME} v{APP_VERSION}")
        self.root.geometry("1200x800")
        self.root.minsize(1000, 700)

        self.cfg = load_config()
        self.excel = ExcelReader()
        self.excel_names = []          # 当前 Excel 有效条目
        self.excel_stats = {}
        self.target_paths = []         # 目标路径列表
        self.scan_engine = None
        self.scan_thread = None
        self.delete_engine = None
        self.delete_thread = None
        self.result_queue = Queue()
        self.result_items = []         # 已加入表格的结果 dict
        self.checked = set()           # 已勾选的 path 集合（勾选真相来源）
        self.whitelist = self.cfg.get("whitelist_paths", [])
        self.delete_log = []           # 删除日志（导出用）
        self._after_id = None

        # 当前匹配设置
        self.delete_type = self.cfg.get("delete_type", "all")
        self.match_mode = self.cfg.get("match_mode", "exact")
        self.ignore_case = self.cfg.get("ignore_case", True)
        self.recursive = self.cfg.get("recursive", True)
        self.max_depth = self.cfg.get("max_depth", 0)  # 0 = 无限制
        self.exclude_text = ",".join(self.cfg.get("exclude_paths", []))

        self.setup_ui()
        self.after_id_drain = None
        self._restore_config()
        self._start_drain()

    # ---------------- UI 构建 ----------------
    def setup_ui(self):
        # 顶部工具栏
        top = ttk.Frame(self.root)
        top.pack(side=tk.TOP, fill=tk.X)
        ttk.Label(top, text=APP_NAME, font=("Microsoft YaHei", 14, "bold")).pack(side=tk.LEFT, padx=10, pady=6)
        ttk.Button(top, text="帮助", command=self.show_help).pack(side=tk.RIGHT, padx=6)
        ttk.Button(top, text="关于", command=self.show_about).pack(side=tk.RIGHT, padx=6)

        # 主区域：左配置 右结果
        main = ttk.PanedWindow(self.root, orient=tk.HORIZONTAL)
        main.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=6, pady=6)

        left = ttk.Frame(main, width=420)
        right = ttk.Frame(main)
        main.add(left, weight=0)
        main.add(right, weight=1)

        self._build_left(left)
        self._build_right(right)
        self._build_log_bar()

    def _build_left(self, parent):
        # ① 数据源配置
        f1 = ttk.LabelFrame(parent, text="① 数据源配置（Excel 清单）", padding=8)
        f1.pack(fill=tk.X, pady=(0, 6))

        ttk.Label(f1, text="Excel 文件:").pack(anchor=tk.W)
        ex_row = ttk.Frame(f1)
        ex_row.pack(fill=tk.X, pady=2)
        self.excel_path_var = tk.StringVar()
        ttk.Entry(ex_row, textvariable=self.excel_path_var).pack(side=tk.LEFT, fill=tk.X, expand=True)
        ttk.Button(ex_row, text="浏览", command=self.browse_excel).pack(side=tk.LEFT, padx=4)

        sheet_row = ttk.Frame(f1)
        sheet_row.pack(fill=tk.X, pady=2)
        ttk.Label(sheet_row, text="工作表:").pack(side=tk.LEFT)
        self.sheet_var = tk.StringVar()
        self.sheet_combo = ttk.Combobox(sheet_row, textvariable=self.sheet_var, state="readonly", width=18)
        self.sheet_combo.pack(side=tk.LEFT, padx=4)
        self.sheet_combo.bind("<<ComboboxSelected>>", lambda e: self.on_sheet_changed())
        ttk.Label(sheet_row, text="数据列:").pack(side=tk.LEFT, padx=(8, 0))
        self.col_var = tk.StringVar()
        self.col_combo = ttk.Combobox(sheet_row, textvariable=self.col_var, state="readonly", width=18)
        self.col_combo.pack(side=tk.LEFT, padx=4)
        self.col_combo.bind("<<ComboboxSelected>>", lambda e: self.on_column_changed())

        self.excel_stat_var = tk.StringVar(value="有效条目: 0 条")
        ttk.Label(f1, textvariable=self.excel_stat_var, foreground="blue").pack(anchor=tk.W, pady=2)
        ttk.Button(f1, text="预览前 20 行", command=self.preview_excel).pack(anchor=tk.W, pady=2)

        # ② 扫描配置
        f2 = ttk.LabelFrame(parent, text="② 扫描配置（目标路径）", padding=8)
        f2.pack(fill=tk.X, pady=(0, 6))

        ttk.Label(f2, text="目标路径:").pack(anchor=tk.W)
        path_list_frame = ttk.Frame(f2)
        path_list_frame.pack(fill=tk.X, pady=2)
        self.path_listbox = tk.Listbox(path_list_frame, height=4)
        self.path_listbox.pack(side=tk.LEFT, fill=tk.X, expand=True)
        path_scroll = ttk.Scrollbar(path_list_frame, orient=tk.VERTICAL, command=self.path_listbox.yview)
        self.path_listbox.configure(yscrollcommand=path_scroll.set)
        path_scroll.pack(side=tk.RIGHT, fill=tk.Y)

        pbtn = ttk.Frame(f2)
        pbtn.pack(fill=tk.X, pady=2)
        ttk.Button(pbtn, text="添加路径", command=self.add_target_path).pack(side=tk.LEFT, padx=2)
        ttk.Button(pbtn, text="删除选中", command=self.remove_target_path).pack(side=tk.LEFT, padx=2)
        ttk.Button(pbtn, text="清空", command=self.clear_target_paths).pack(side=tk.LEFT, padx=2)

        # 排除路径(高级)
        ttk.Label(f2, text="排除目录(逗号分隔,如 .git,node_modules):").pack(anchor=tk.W, pady=(4, 0))
        self.exclude_var = tk.StringVar(value=self.exclude_text)
        ttk.Entry(f2, textvariable=self.exclude_var).pack(fill=tk.X, pady=2)
        ttk.Label(f2, text="白名单(永不被删,逗号分隔):").pack(anchor=tk.W, pady=(2, 0))
        self.whitelist_var = tk.StringVar(value=",".join(self.whitelist))
        ttk.Entry(f2, textvariable=self.whitelist_var).pack(fill=tk.X, pady=2)

        # ③ 匹配设置
        f3 = ttk.LabelFrame(parent, text="③ 匹配设置", padding=8)
        f3.pack(fill=tk.X, pady=(0, 6))

        ttk.Label(f3, text="删除类型:").pack(anchor=tk.W)
        dt_frame = ttk.Frame(f3)
        dt_frame.pack(fill=tk.X, pady=2)
        self.dt_var = tk.StringVar(value=self.delete_type)
        for val, lab in [("file", "仅文件"), ("folder", "仅文件夹"), ("all", "全部")]:
            ttk.Radiobutton(dt_frame, text=lab, variable=self.dt_var, value=val,
                            command=self.on_settings_changed).pack(side=tk.LEFT, padx=4)

        ttk.Label(f3, text="匹配模式:").pack(anchor=tk.W, pady=(4, 0))
        mm_frame = ttk.Frame(f3)
        mm_frame.pack(fill=tk.X, pady=2)
        self.mm_var = tk.StringVar(value=self.match_mode)
        for val, lab in [("exact", "精确"), ("contains", "包含"), ("regex", "正则")]:
            ttk.Radiobutton(mm_frame, text=lab, variable=self.mm_var, value=val,
                            command=self.on_settings_changed).pack(side=tk.LEFT, padx=4)

        opt = ttk.Frame(f3)
        opt.pack(fill=tk.X, pady=2)
        self.rec_var = tk.BooleanVar(value=self.recursive)
        ttk.Checkbutton(opt, text="递归子目录", variable=self.rec_var,
                        command=self.on_settings_changed).pack(side=tk.LEFT, padx=4)
        self.ic_var = tk.BooleanVar(value=self.ignore_case)
        ttk.Checkbutton(opt, text="忽略大小写", variable=self.ic_var,
                        command=self.on_settings_changed).pack(side=tk.LEFT, padx=4)

        depth_frame = ttk.Frame(f3)
        depth_frame.pack(fill=tk.X, pady=2)
        ttk.Label(depth_frame, text="最大深度(0=无限):").pack(side=tk.LEFT)
        self.depth_var = tk.IntVar(value=self.max_depth)
        ttk.Spinbox(depth_frame, from_=0, to=20, width=5,
                    textvariable=self.depth_var,
                    command=self.on_settings_changed).pack(side=tk.LEFT, padx=4)

        # 操作按钮
        act = ttk.Frame(parent)
        act.pack(fill=tk.X, pady=4)
        self.scan_btn = ttk.Button(act, text="扫描预览", command=self.start_scan)
        self.scan_btn.pack(side=tk.LEFT, padx=4)
        self.scan_stop_btn = ttk.Button(act, text="取消扫描", command=self.stop_scan, state=tk.DISABLED)
        self.scan_stop_btn.pack(side=tk.LEFT, padx=4)
        ttk.Button(act, text="清空结果", command=self.clear_results).pack(side=tk.LEFT, padx=4)

    def _build_right(self, parent):
        ctrl = ttk.Frame(parent)
        ctrl.pack(fill=tk.X, pady=(0, 4))
        self.result_stat_var = tk.StringVar(value="匹配：文件 0 | 文件夹 0 | 总大小 0")
        ttk.Label(ctrl, textvariable=self.result_stat_var, foreground="green").pack(side=tk.LEFT)
        ttk.Button(ctrl, text="全选", command=self.select_all).pack(side=tk.RIGHT, padx=2)
        ttk.Button(ctrl, text="全不选", command=self.select_none).pack(side=tk.RIGHT, padx=2)
        ttk.Label(ctrl, text="筛选:").pack(side=tk.RIGHT, padx=(8, 2))
        self.filter_var = tk.StringVar()
        self.filter_var.trace_add("write", lambda *a: self.apply_filter())
        ttk.Entry(ctrl, textvariable=self.filter_var, width=16).pack(side=tk.RIGHT)

        # 结果表格
        table_frame = ttk.Frame(parent)
        table_frame.pack(fill=tk.BOTH, expand=True)
        cols = ("选择", "#", "类型", "名称", "完整路径", "大小", "修改时间", "匹配条目")
        self.tree = ttk.Treeview(table_frame, columns=cols, show="headings", selectmode="extended")
        widths = {"选择": 40, "#": 50, "类型": 70, "名称": 180, "完整路径": 280,
                  "大小": 90, "修改时间": 130, "匹配条目": 120}
        for c in cols:
            anchor = tk.W if c in ("名称", "完整路径", "匹配条目") else tk.CENTER
            self.tree.heading(c, text=c, anchor=anchor,
                              command=lambda col=c: self.sort_by_column(col))
            self.tree.column(c, width=widths[c], anchor=anchor)
        yscroll = ttk.Scrollbar(table_frame, orient=tk.VERTICAL, command=self.tree.yview)
        self.tree.configure(yscrollcommand=yscroll.set)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        yscroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.bind("<Button-1>", self.on_tree_click)
        # 右键菜单
        self.ctx_menu = tk.Menu(self.tree, tearoff=0)
        self.ctx_menu.add_command(label="打开所在文件夹", command=self.ctx_open_folder)
        self.ctx_menu.add_command(label="复制路径", command=self.ctx_copy_path)
        self.ctx_menu.add_command(label="排除此项", command=self.ctx_exclude)
        self.tree.bind("<Button-3>", self.on_tree_right_click)

    def _build_log_bar(self):
        bottom = ttk.Frame(self.root)
        bottom.pack(side=tk.BOTTOM, fill=tk.X, padx=6, pady=6)

        log_top = ttk.Frame(bottom)
        log_top.pack(fill=tk.X)
        ttk.Label(log_top, text="④ 操作日志").pack(side=tk.LEFT)
        ttk.Button(log_top, text="清空", command=lambda: self.log_text.delete("1.0", tk.END)).pack(side=tk.RIGHT, padx=2)
        ttk.Button(log_top, text="导出日志", command=self.export_log).pack(side=tk.RIGHT, padx=2)

        self.log_text = scrolledtext.ScrolledText(bottom, height=7, state=tk.DISABLED)
        self.log_text.pack(fill=tk.X)

        # 删除执行区
        del_frame = ttk.Frame(bottom)
        del_frame.pack(fill=tk.X, pady=(4, 0))
        self.progress = ttk.Progressbar(del_frame, mode="determinate")
        self.progress.pack(fill=tk.X, pady=2)
        self.del_btn = ttk.Button(del_frame, text="执行删除（移至回收站）",
                                  command=self.start_delete)
        self.del_btn.pack(side=tk.RIGHT, pady=2)

    # ---------------- 配置恢复 ----------------
    def _restore_config(self):
        p = self.cfg.get("last_excel_path", "")
        if p and os.path.isfile(p):
            self.excel_path_var.set(p)
            self.load_excel_file(p, silent=True)
        for tp in self.cfg.get("last_target_paths", []):
            if tp and os.path.isdir(tp):
                self.target_paths.append(tp)
                self.path_listbox.insert(tk.END, tp)

    # ---------------- Excel 相关 ----------------
    def browse_excel(self):
        path = filedialog.askopenfilename(
            title="选择 Excel 文件",
            filetypes=[("Excel files", "*.xlsx *.xls"), ("All", "*.*")])
        if not path:
            return
        self.excel_path_var.set(path)
        self.load_excel_file(path)

    def load_excel_file(self, path, silent=False):
        if not OPENPYXL_OK and path.lower().endswith(".xlsx"):
            messagebox.showerror("依赖缺失", "未安装 openpyxl，无法读取 .xlsx 文件。\n请执行：pip install openpyxl")
            return
        if not self.excel.open_file(path):
            if not silent:
                messagebox.showerror("错误", self.excel.last_error)
            return
        self.sheet_combo["values"] = self.excel.sheet_names
        if self.excel.sheet_names:
            self.sheet_var.set(self.excel.sheet_names[0])
            self.on_sheet_changed()
        self.persist_config()

    def on_sheet_changed(self):
        name = self.sheet_var.get()
        if not name:
            return
        headers = self.excel.load_sheet_headers(name)
        self.col_combo["values"] = headers
        if headers:
            # 恢复上次列或默认第一列
            idx = self.cfg.get("last_column_index", 0)
            if idx < len(headers):
                self.col_var.set(headers[idx])
            else:
                self.col_var.set(headers[0])
            self.on_column_changed()

    def on_column_changed(self):
        sel = self.col_var.get()
        if not sel:
            return
        # 解析列字母
        letter = sel.split(":")[0].strip()
        idx = col_letter_to_index(letter)
        self.excel.selected_col_index = idx
        self.read_column_data()

    def read_column_data(self):
        sheet = self.sheet_var.get()
        idx = self.excel.selected_col_index
        if not sheet or idx is None:
            return
        stats = self.excel.read_column(sheet, idx)
        self.excel_stats = stats
        self.excel_names = stats["valid"]
        total = len(self.excel_names)
        self.excel_stat_var.set(
            f"有效条目: {total} 条（原始 {stats['raw_count']}，空 {stats['empty_count']}，"
            f"重复 {stats['dup_count']}，超长/非法 {stats['skipped_long'] + stats['skipped_illegal']}）")
        if total == 0 and self.excel.last_error:
            messagebox.showwarning("提示", self.excel.last_error)
        self.persist_config()

    def preview_excel(self):
        if not self.excel_stats.get("preview"):
            messagebox.showinfo("预览", "请先选择 Excel 并读取列数据。")
            return
        win = tk.Toplevel(self.root)
        win.title("Excel 前 20 行预览")
        win.geometry("400x400")
        tv = ttk.Treeview(win, columns=("#", "内容"), show="headings")
        tv.heading("#", text="#")
        tv.heading("内容", text="内容")
        tv.column("#", width=40)
        tv.column("内容", width=340)
        tv.pack(fill=tk.BOTH, expand=True)
        for i, v in enumerate(self.excel_stats["preview"], 1):
            tv.insert("", tk.END, values=(i, v))

    # ---------------- 目标路径 ----------------
    def add_target_path(self):
        path = filedialog.askdirectory(title="选择目标目录（可多选，重复调用添加）")
        if not path:
            return
        if is_drive_root(path):
            if not messagebox.askyesno("高风险警告",
                                        f"您选择的是磁盘根目录：{path}\n"
                                        "误删可能导致系统/数据严重损失！\n确认继续？"):
                return
        if path not in self.target_paths:
            self.target_paths.append(path)
            self.path_listbox.insert(tk.END, path)
            self.persist_config()

    def remove_target_path(self):
        sel = self.path_listbox.curselection()
        if not sel:
            return
        i = sel[0]
        self.target_paths.pop(i)
        self.path_listbox.delete(i)

    def clear_target_paths(self):
        self.target_paths.clear()
        self.path_listbox.delete(0, tk.END)

    # ---------------- 设置变更 ----------------
    def on_settings_changed(self):
        self.delete_type = self.dt_var.get()
        self.match_mode = self.mm_var.get()
        self.ignore_case = self.ic_var.get()
        self.recursive = self.rec_var.get()
        self.max_depth = self.depth_var.get()
        self.persist_config()

    def persist_config(self):
        cfg = {
            "last_excel_path": self.excel_path_var.get(),
            "last_sheet_name": self.sheet_var.get(),
            "last_column_index": self.excel.selected_col_index,
            "last_target_paths": self.target_paths,
            "delete_type": self.dt_var.get(),
            "match_mode": self.mm_var.get(),
            "recursive": self.rec_var.get(),
            "max_depth": self.depth_var.get(),
            "ignore_case": self.ic_var.get(),
            "exclude_paths": [x.strip() for x in self.exclude_var.get().split(",") if x.strip()],
            "whitelist_paths": [x.strip() for x in self.whitelist_var.get().split(",") if x.strip()],
        }
        save_config(cfg)

    # ---------------- 扫描 ----------------
    def start_scan(self):
        if not self.excel_names:
            messagebox.showwarning("提示", "Excel 有效条目为 0，无法扫描（请检查数据源）。")
            return
        if not self.target_paths:
            messagebox.showwarning("提示", "请先添加至少一个目标路径。")
            return
        self.on_settings_changed()
        self.whitelist = [x.strip() for x in self.whitelist_var.get().split(",") if x.strip()]
        exclude = [x.strip() for x in self.exclude_var.get().split(",") if x.strip()]

        self.clear_results()
        self.scan_engine = ScanEngine()
        self.scan_btn.config(state=tk.DISABLED)
        self.scan_stop_btn.config(state=tk.NORMAL)
        self.del_btn.config(state=tk.DISABLED)
        self.log("INFO", f"开始扫描，目标路径 {len(self.target_paths)} 个，模式 "
                         f"{MATCH_MODE_LABELS.get(self.match_mode)}/{DELETE_TYPE_LABELS.get(self.delete_type)}")

        def worker():
            scanned, matched = self.scan_engine.scan(
                names=self.excel_names,
                target_paths=self.target_paths,
                delete_type=self.delete_type,
                match_mode=self.match_mode,
                ignore_case=self.ignore_case,
                recursive=self.recursive,
                max_depth=self.max_depth if self.max_depth > 0 else None,
                exclude_dirs=exclude,
                whitelist=self.whitelist,
                result_queue=self.result_queue,
                progress_cb=self.on_scan_progress,
                log_cb=self.log,
            )
            self.root.after(0, lambda: self.on_scan_finished(scanned, matched))

        self.scan_thread = threading.Thread(target=worker, daemon=True)
        self.scan_thread.start()

    def on_scan_progress(self, done, matched):
        # 进度条用匹配数粗略表示（无法预知总数）
        self.progress["mode"] = "indeterminate"
        if not self.progress["value"]:
            self.progress.start(10)

    def stop_scan(self):
        if self.scan_engine:
            self.scan_engine.stop()
        self.log("INFO", "已请求取消扫描…")

    def on_scan_finished(self, scanned, matched):
        self.scan_btn.config(state=tk.NORMAL)
        self.scan_stop_btn.config(state=tk.DISABLED)
        self.progress.stop()
        self.progress["value"] = 0
        if self.scan_engine and self.scan_engine.stop_flag:
            self.log("INFO", f"扫描已取消。已匹配 {len(self.result_items)} 项。")
        else:
            self.log("INFO", f"扫描完成，扫描 {scanned} 项，匹配 {matched} 项。")
        self.update_result_stats()
        if self.result_items:
            self.del_btn.config(state=tk.NORMAL)

    def _start_drain(self):
        def drain():
            try:
                batch = []
                while True:
                    item = self.result_queue.get_nowait()
                    batch.append(item)
                    if len(batch) >= 200:
                        break
                if batch:
                    self._insert_results(batch)
            except Empty:
                pass
            self.after_id_drain = self.root.after(120, drain)
        self.after_id_drain = self.root.after(120, drain)

    def _insert_results(self, items):
        for it in items:
            self.result_items.append(it)
            self.checked.add(it["path"])  # 默认全选
            self.tree.insert("", tk.END, values=(
                CHECKED,
                len(self.result_items),
                "文件夹" if it["is_dir"] else "文件",
                it["name"],
                it["path"],
                fmt_size(it["size"]),
                it["mtime"],
                it["matched"],
            ))
        self.update_result_stats()

    # ---------------- 结果表格操作 ----------------
    def update_result_stats(self):
        files = sum(1 for it in self.result_items if not it["is_dir"])
        folders = sum(1 for it in self.result_items if it["is_dir"])
        total = sum((it["size"] or 0) for it in self.result_items if it["size"])
        self.result_stat_var.set(
            f"匹配：文件 {files} | 文件夹 {folders} | 总大小 {fmt_size(total)}")

    def select_all(self):
        for it in self.result_items:
            self.checked.add(it["path"])
        self.rebuild_tree()

    def select_none(self):
        self.checked.clear()
        self.rebuild_tree()

    def on_tree_click(self, event):
        region = self.tree.identify_region(event.x, event.y)
        col = self.tree.identify_column(event.x)
        row = self.tree.identify_row(event.y)
        if region == "cell" and col == "#1" and row:
            path = self.tree.item(row, "values")[4]
            checked = path in self.checked
            if checked:
                self.checked.discard(path)
            else:
                self.checked.add(path)
            vals = list(self.tree.item(row, "values"))
            vals[0] = UNCHECKED if checked else CHECKED
            self.tree.item(row, values=vals)
            return "break"

    def rebuild_tree(self):
        """依据 self.result_items + self.checked + 筛选关键字重建表格"""
        kw = self.filter_var.get().strip().lower()
        self.tree.delete(*self.tree.get_children())
        idx = 0
        for it in self.result_items:
            if kw:
                text = " ".join([it["name"], it["path"], it["matched"]]).lower()
                if kw not in text:
                    continue
            idx += 1
            self.tree.insert("", tk.END, values=(
                CHECKED if it["path"] in self.checked else UNCHECKED,
                idx,
                "文件夹" if it["is_dir"] else "文件",
                it["name"],
                it["path"],
                fmt_size(it["size"]),
                it["mtime"],
                it["matched"],
            ))

    def on_tree_right_click(self, event):
        row = self.tree.identify_row(event.y)
        if row:
            self.tree.selection_set(row)
            self.ctx_menu.post(event.x_root, event.y_root)

    def ctx_open_folder(self):
        sel = self.tree.selection()
        if not sel:
            return
        path = self.tree.item(sel[0], "values")[4]
        folder = os.path.dirname(path)
        try:
            os.startfile(folder)
        except Exception as e:
            messagebox.showerror("错误", str(e))

    def ctx_copy_path(self):
        sel = self.tree.selection()
        if not sel:
            return
        path = self.tree.item(sel[0], "values")[4]
        self.root.clipboard_clear()
        self.root.clipboard_append(path)

    def ctx_exclude(self):
        sel = self.tree.selection()
        if not sel:
            return
        path = self.tree.item(sel[0], "values")[4]
        folder = os.path.dirname(path)
        # 将父目录加入白名单
        self.whitelist.append(folder)
        self.whitelist = list(dict.fromkeys(self.whitelist))
        self.whitelist_var.set(",".join(self.whitelist))
        self.persist_config()
        # 同步 result_items 与勾选
        self.result_items = [it for it in self.result_items if it["path"] != path]
        self.checked.discard(path)
        self.rebuild_tree()
        self.update_result_stats()
        self.log("INFO", f"已排除并加入白名单：{folder}")

    def apply_filter(self, *args):
        self.rebuild_tree()

    def sort_by_column(self, col):
        items = [(self.tree.set(k, col), k) for k in self.tree.get_children("")]
        if col == "大小":
            def keyf(x):
                try:
                    return float(str(x[0]).replace(" B", "").replace(" KB", "").replace(" MB", "").replace(" GB", "").replace(" TB", "").replace("—", "0").split(" ")[0]) if x[0] != "—" else 0
                except Exception:
                    return 0
            items.sort(key=keyf)
        elif col == "#":
            items.sort(key=lambda x: int(x[0]) if str(x[0]).isdigit() else 0)
        else:
            items.sort(key=lambda x: str(x[0]).lower())
        for idx, (_, k) in enumerate(items):
            self.tree.move(k, "", idx)

    def clear_results(self):
        self.tree.delete(*self.tree.get_children())
        self.result_items = []
        self.checked.clear()
        self.result_stat_var.set("匹配：文件 0 | 文件夹 0 | 总大小 0")

    # ---------------- 删除执行 ----------------
    def get_selected_items(self):
        return [it for it in self.result_items if it["path"] in self.checked]

    def start_delete(self):
        items = self.get_selected_items()
        if not items:
            messagebox.showinfo("提示", "请先勾选要删除的项。")
            return
        files = sum(1 for it in items if not it["is_dir"])
        folders = sum(1 for it in items if it["is_dir"])
        total = sum((it["size"] or 0) for it in items if it["size"])
        # 二次确认对话框
        confirm = self.show_confirm_dialog(files, folders, total)
        if not confirm:
            return

        self.delete_log = []
        self.delete_engine = DeleteEngine()
        self.del_btn.config(state=tk.DISABLED)
        self.progress["mode"] = "determinate"
        self.progress["value"] = 0
        self.log("INFO", f"开始执行删除，共 {len(items)} 项（移至回收站）。")

        def worker():
            start = time.time()
            report = self.delete_engine.delete_items(
                items=items,
                whitelist=self.whitelist,
                progress_cb=lambda d, t: self.root.after(0, lambda: self.on_delete_progress(d, t)),
                item_cb=lambda r: self.root.after(0, lambda: self.on_delete_item(r)),
                log_cb=self.log,
            )
            elapsed = time.time() - start
            self.root.after(0, lambda: self.on_delete_finished(report, elapsed))

        self.delete_thread = threading.Thread(target=worker, daemon=True)
        self.delete_thread.start()

    def on_delete_progress(self, done, total):
        if total > 0:
            self.progress["value"] = int(done / total * 100)
            self.progress.update()

    def on_delete_item(self, rec):
        self.delete_log.append(rec)
        self.result_items = [it for it in self.result_items if it["path"] != rec["path"]]
        self.checked.discard(rec["path"])
        self.rebuild_tree()
        self.update_result_stats()

    def on_delete_finished(self, report, elapsed):
        self.del_btn.config(state=tk.NORMAL if self.result_items else tk.DISABLED)
        self.progress["value"] = 100
        self.log("INFO", f"删除完成：成功 {report['success']}，失败 {report['failed']}，"
                         f"跳过 {report['skipped']}，释放 {fmt_size(report['freed'])}，"
                         f"耗时 {int(elapsed)} 秒。")
        self.show_report_dialog(report, elapsed)

    # ---------------- 对话框 ----------------
    def show_confirm_dialog(self, files, folders, total):
        win = tk.Toplevel(self.root)
        win.title("⚠ 确认删除")
        win.geometry("400x300")
        win.resizable(False, False)
        win.transient(self.root)
        win.grab_set()
        result = {"ok": False}

        ttk.Label(win, text="即将删除以下内容到回收站：", font=("Microsoft YaHei", 11)).pack(pady=(12, 6))
        info = tk.StringVar(value=f"· 文件：{files} 个\n· 文件夹：{folders} 个\n· 总大小：{fmt_size(total)}")
        ttk.Label(win, textvariable=info, justify=tk.LEFT).pack(padx=20, anchor=tk.W)
        ttk.Label(win, text="删除后可通过系统回收站恢复。", foreground="gray").pack(padx=20, pady=(6, 0))

        ack_var = tk.BooleanVar(value=False)
        ack = ttk.Checkbutton(win, text="我已确认以上内容无误", variable=ack_var)
        ack.pack(padx=20, pady=(10, 0))

        def do_cancel():
            result["ok"] = False
            win.destroy()

        def do_confirm():
            if ack_var.get():
                result["ok"] = True
                win.destroy()

        btn_frame = ttk.Frame(win)
        btn_frame.pack(side=tk.BOTTOM, fill=tk.X, pady=10)
        ttk.Button(btn_frame, text="取消", command=do_cancel).pack(side=tk.RIGHT, padx=10)
        confirm_btn = ttk.Button(btn_frame, text="确认删除", command=do_confirm, state=tk.DISABLED)
        confirm_btn.pack(side=tk.RIGHT, padx=10)

        def on_ack(*a):
            confirm_btn.config(state=tk.NORMAL if ack_var.get() else tk.DISABLED)
        ack_var.trace_add("write", on_ack)

        win.wait_window()
        return result["ok"]

    def show_report_dialog(self, report, elapsed):
        win = tk.Toplevel(self.root)
        win.title("✓ 删除完成")
        win.geometry("460x360")
        win.transient(self.root)
        win.grab_set()

        ttk.Label(win, text="✓ 删除完成", font=("Microsoft YaHei", 14, "bold")).pack(pady=(12, 6))
        text = (f"成功删除：{report['success']} 项\n"
                f"删除失败：{report['failed']} 项\n"
                f"跳过：{report['skipped']} 项\n"
                f"释放空间：{fmt_size(report['freed'])}\n"
                f"耗时：{int(elapsed)} 秒")
        ttk.Label(win, text=text, justify=tk.LEFT).pack(padx=20, anchor=tk.W)

        fails = [r for r in self.delete_log if r["result"] == "failed"]
        if fails:
            ttk.Label(win, text="失败详情：", foreground="red").pack(anchor=tk.W, padx=20, pady=(6, 0))
            lb = tk.Listbox(win, height=6)
            for r in fails:
                lb.insert(tk.END, f"· {r['path']}（{r['error']}）")
            lb.pack(fill=tk.X, padx=20, pady=2)

        ttk.Label(win, text="所有删除的文件可在系统回收站中恢复。", foreground="gray").pack(padx=20, pady=6)

        btn = ttk.Frame(win)
        btn.pack(side=tk.BOTTOM, fill=tk.X, pady=10)
        ttk.Button(btn, text="导出日志", command=self.export_log).pack(side=tk.RIGHT, padx=8)
        ttk.Button(btn, text="打开回收站",
                   command=lambda: self.open_recycle_bin()).pack(side=tk.RIGHT, padx=8)
        ttk.Button(btn, text="关闭", command=win.destroy).pack(side=tk.RIGHT, padx=8)

    def open_recycle_bin(self):
        try:
            os.startfile("explorer.exe", "::{645FF040-5081-101B-9F08-00AA002F954E}")
        except Exception:
            try:
                os.startfile("shell:RecycleBinFolder")
            except Exception:
                pass

    # ---------------- 日志 ----------------
    def log(self, level, msg):
        ts = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        line = f"[{ts}] [{level}] {msg}\n"
        self.log_text.configure(state=tk.NORMAL)
        self.log_text.insert(tk.END, line)
        self.log_text.configure(state=tk.DISABLED)
        self.log_text.see(tk.END)

    def export_log(self):
        if not self.delete_log:
            messagebox.showinfo("提示", "暂无删除日志可导出。")
            return
        path = filedialog.asksaveasfilename(
            title="导出日志",
            defaultextension=".txt",
            filetypes=[("文本文件", "*.txt"), ("Excel", "*.xlsx")])
        if not path:
            return
        if path.lower().endswith(".xlsx"):
            if not OPENPYXL_OK:
                messagebox.showerror("错误", "需要 openpyxl 才能导出 xlsx")
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
        messagebox.showinfo("完成", f"日志已导出：{path}")

    # ---------------- 帮助/关于 ----------------
    def show_help(self):
        txt = (
            "使用步骤：\n"
            "1. 选择 Excel 清单，指定工作表与数据列（默认第一列）。\n"
            "2. 添加目标扫描路径，可设置排除目录与白名单。\n"
            "3. 选择删除类型/匹配模式，点击「扫描预览」。\n"
            "4. 在右侧结果中勾选要删除的项（默认全选）。\n"
            "5. 点击「执行删除」，确认后移至回收站（可恢复）。\n\n"
            "安全机制：系统目录保护、白名单、二次确认、回收站恢复、全量日志。"
        )
        messagebox.showinfo("帮助", txt)

    def show_about(self):
        messagebox.showinfo("关于",
            f"{APP_NAME}\n版本 {APP_VERSION}\n\n"
            "Excel 清单驱动的批量文件清理工具。\n"
            "删除到回收站，安全可恢复。\n\n"
            "百宝箱 (Toolbox) 组件")


# ==================== 程序入口 ====================
def main():
    """百宝箱入口函数"""
    if not OPENPYXL_OK:
        print("警告: openpyxl 未安装，无法读取 .xlsx（pip install openpyxl）")
    if not SEND2TRASH_OK:
        print("警告: send2trash 未安装，删除将不可恢复（pip install send2trash）")
    root = tk.Tk()
    try:
        root.iconbitmap()
    except Exception:
        pass
    app = FileJanitorApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
