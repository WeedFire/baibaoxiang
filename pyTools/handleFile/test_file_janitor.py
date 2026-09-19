# -*- coding: utf-8 -*-
"""
文件清理助手 (FileJanitor) 核心逻辑单元测试。

覆盖：工具函数、Excel 读取清洗、扫描匹配引擎、删除引擎（含白名单/系统保护）。
不依赖 GUI；直接 import 纯逻辑类与函数。
"""
import os
import sys
import tempfile
import shutil
import pytest

# 将本目录加入 path，便于 import file_janitor（其内部会 import PyQt5，本机已安装）
HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from file_janitor import (  # noqa: E402
    fmt_size,
    col_letter_to_index,
    normalize_path,
    is_protected_path,
    is_drive_root,
    ExcelReader,
    ScanEngine,
    DeleteEngine,
    ILLEGAL_CHARS,
)


# ---------------- 删除路径归一化回归测试 ----------------
# 回归背景：send2trash(Windows) 会自动给路径拼 `\\?\` 长路径前缀，而该前缀
# 只接受全反斜杠的本地绝对路径。Excel 里常见 `D:/temp/x` 这种正斜杠写法，
# 一旦混入就会报 [Errno 3] 系统找不到指定的路径（'\\?\D:/temp/x'）。
def test_delete_engine_deletes_forward_slash_path():
    """含正斜杠的路径必须能正常删除（删除前归一化为原生反斜杠路径）。

    注意：不能用 tmp_path（位于 AppData，属系统保护目录会被正确跳过），
    改用用户主目录下的临时目录。"""
    from file_janitor import DeleteEngine, is_protected_path, to_native_path

    d = tempfile.mkdtemp(prefix="fj_del_test_", dir=os.path.expanduser("~"))
    try:
        f = os.path.join(d, "数据2024.tmp")
        with open(f, "w", encoding="utf-8") as fp:
            fp.write("x")
        mixed = to_native_path(d).replace("\\", "/") + "/数据2024.tmp"
        assert "/" in mixed and not is_protected_path(mixed)

        eng = DeleteEngine()
        recs = []
        eng.delete_items(
            [{"path": mixed, "is_dir": False, "size": 1}], [],
            lambda dd, t: None, recs.append, lambda *a: None)
        assert recs and recs[0]["result"] == "success", recs
        assert not os.path.exists(f)
    finally:
        shutil.rmtree(d, ignore_errors=True)


def test_to_native_path_normalizes_separators():
    """to_native_path 统一为原生反斜杠绝对路径，去掉引号与空白。"""
    from file_janitor import to_native_path

    if os.name != "nt":
        pytest.skip("仅 Windows 需要反斜杠归一化")
    assert to_native_path("D:/temp/test_data") == "D:\\temp\\test_data"
    assert to_native_path('  "D:/temp/x"  ') == "D:\\temp\\x"
    assert to_native_path("  D:\\a/b\\c  ") == "D:\\a\\b\\c"


# ---------------- GUI 构造回归测试 ----------------
# 回归背景：_radio_row 曾“先 connect 后 setChecked(True)”，导致构造期
# toggled 信号回调 on_settings_changed 时 self.dt_group / mm_group 尚未
# 赋值，抛 AttributeError（PyQt5 槽异常 → 进程原生崩溃 0xC0000409）。
def test_main_window_constructs_without_slot_error():
    """主窗口必须能完整构造，且单选组属性存在、取值合法。"""
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PyQt5.QtWidgets import QApplication
    from file_janitor import FileJanitorApp

    app = QApplication.instance() or QApplication([])
    w = FileJanitorApp()
    assert w.dt_group and w.mm_group
    assert w._group_value(w.dt_group) in ("file", "folder", "all")
    assert w._group_value(w.mm_group) in ("exact", "contains", "regex")
    # 构造期不应误触发持久化后的设置状态
    assert isinstance(w.delete_type, str) and isinstance(w.match_mode, str)
    w.close()
    app.quit()


def test_result_table_rows_complete_with_sorting_enabled():
    """回归：排序开启时逐行 insertRow+setItem 会导致行被实时重排、
    单元格散乱（表现为表格只剩最后一行有数据）。
    扫描结果填充期间必须临时关闭排序，填充完成后再恢复。"""
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    from PyQt5.QtCore import Qt
    from PyQt5.QtWidgets import QApplication
    from file_janitor import FileJanitorApp

    app = QApplication.instance() or QApplication([])
    w = FileJanitorApp()
    # 模拟用户点击过表头（排序指示器生效）后逐条接收扫描结果
    w.table.sortByColumn(0, Qt.AscendingOrder)
    try:
        for i in range(5):
            w.on_result({
                "path": f"D:/t/item{i}.txt",
                "name": f"item{i}.txt",
                "is_dir": False,
                "size": 10,
                "mtime": "2026-09-19 10:00:00",
                "matched": f"数据{i}",
            })
        assert w.table.rowCount() == 5
        for r in range(5):
            for col, label in ((3, "名称"), (4, "完整路径"), (6, "修改时间")):
                it = w.table.item(r, col)
                assert it is not None and it.text(), f"第 {r+1} 行「{label}」列为空"
                assert it.text() != "", f"第 {r+1} 行「{label}」列为空文本"
        # 每行路径应互不相同（未发生错位覆盖）
        paths = {w.table.item(r, 4).text() for r in range(5)}
        assert len(paths) == 5
    finally:
        w.close()
        app.quit()


# ==================== 工具函数 ====================
class TestUtils:
    def test_fmt_size_handles_none(self):
        assert fmt_size(None) == "—"

    def test_fmt_size_bytes(self):
        assert fmt_size(512) == "512 B"

    def test_fmt_size_kilobytes(self):
        assert fmt_size(1536) == "1.50 KB"

    def test_fmt_size_gigabytes(self):
        assert fmt_size(1024 ** 3) == "1.00 GB"

    def test_fmt_size_negative_is_dash(self):
        assert fmt_size(-5) == "—"

    def test_col_letter_to_index(self):
        assert col_letter_to_index("A") == 0
        assert col_letter_to_index("B") == 1
        assert col_letter_to_index("Z") == 25
        assert col_letter_to_index("AA") == 26

    def test_col_letter_to_index_invalid(self):
        assert col_letter_to_index("1") is None
        assert col_letter_to_index("") is None
        assert col_letter_to_index(" a ") == 0

    def test_normalize_path_is_absolute_lowercased(self):
        p = normalize_path("C:/Windows/System32")
        assert os.path.isabs(p)
        assert p == p.lower()

    def test_is_protected_path_windows(self):
        assert is_protected_path(r"C:\Windows\notepad.exe")
        assert is_protected_path(r"C:\Program Files\app\x.exe")
        assert not is_protected_path(r"C:\Users\me\Documents\file.txt")

    def test_is_protected_path_unix(self):
        assert is_protected_path("/usr/bin/ls")
        assert is_protected_path("/etc/passwd")
        assert not is_protected_path("/home/me/file")

    def test_is_drive_root(self):
        assert is_drive_root("C:\\")
        assert is_drive_root("D:")
        assert not is_drive_root(r"C:\Users")
        assert not is_drive_root(r"C:\Users\me")


# ==================== Excel 读取与清洗 ====================
def _make_excel(path, rows):
    """用 openpyxl 写一个临时 xlsx：第一行表头，之后为数据行"""
    from openpyxl import Workbook
    wb = Workbook()
    ws = wb.active
    ws.title = "清单"
    for r in rows:
        ws.append(r)
    wb.save(path)


class TestExcelReader:
    def test_open_xlsx_and_list_sheets(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "list.xlsx")
            _make_excel(p, [["名称", "编号"], ["a.txt", "1"], ["b.txt", "2"]])
            r = ExcelReader()
            assert r.open_file(p) is True
            assert "清单" in r.sheet_names

    def test_read_headers_with_letter_prefix(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "list.xlsx")
            _make_excel(p, [["名称", "编号"], ["a.txt", "1"]])
            r = ExcelReader()
            r.open_file(p)
            headers = r.load_sheet_headers("清单")
            assert headers[0].startswith("A:")
            assert "名称" in headers[0]
            assert headers[1].startswith("B:")
            assert "编号" in headers[1]

    def test_read_column_cleans_blanks_dedup_illegal(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "list.xlsx")
            # A 列：含重复、空值、首尾空格、超长、非法字符
            data = [
                ["名称", "编号"],
                ["report.txt", "1"],
                [" report.txt ", "2"],     # 重复（去空格后）
                ["", "3"],                  # 空
                ["   ", "4"],              # 纯空白
                ["temp.log", "5"],
                ["a" * 300, "6"],          # 超长 -> 跳过
                ['bad<name>.txt', "7"],    # 非法字符 -> 跳过
            ]
            _make_excel(p, data)
            r = ExcelReader()
            r.open_file(p)
            stats = r.read_column("清单", 0)
            assert stats["empty_count"] == 2
            assert stats["dup_count"] == 1
            assert stats["skipped_long"] == 1
            assert stats["skipped_illegal"] == 1
            assert "report.txt" in stats["valid"]
            assert "temp.log" in stats["valid"]
            assert "a" * 300 not in stats["valid"]
            assert "bad<name>.txt" not in stats["valid"]

    def test_open_unsupported_format_returns_false(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, "list.csv")
            with open(p, "w", encoding="utf-8") as f:
                f.write("a,b\n")
            r = ExcelReader()
            assert r.open_file(p) is False
            assert "xlsx" in r.last_error or "xls" in r.last_error


# ==================== 扫描匹配引擎 ====================
def _build_tree(root):
    """构造测试目录：file_a.txt, file_b.log, sub/dir_c"""
    os.makedirs(os.path.join(root, "sub"))
    with open(os.path.join(root, "file_a.txt"), "w") as f:
        f.write("x")
    with open(os.path.join(root, "file_b.log"), "w") as f:
        f.write("y")
    os.makedirs(os.path.join(root, "dir_c"))


class TestScanEngine:
    def test_exact_match_files_only(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["file_a.txt"],
                target_paths=[d],
                delete_type="file",
                match_mode="exact",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[],
                result_cb=items.append,
            )
            names = {os.path.basename(i["path"]) for i in items}
            assert names == {"file_a.txt"}
            assert all(not i["is_dir"] for i in items)

    def test_contains_match_is_case_insensitive(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["FILE"],  # 命中所有以 file 开头的文件名（忽略大小写）
                target_paths=[d],
                delete_type="all",
                match_mode="contains",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[],
                result_cb=items.append,
            )
            names = {os.path.basename(i["path"]) for i in items}
            assert "file_a.txt" in names
            assert "file_b.log" in names

    def test_folder_only_excludes_files(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["dir_c", "file_a.txt"],
                target_paths=[d],
                delete_type="folder",
                match_mode="exact",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[],
                result_cb=items.append,
            )
            assert all(i["is_dir"] for i in items)
            assert {os.path.basename(i["path"]) for i in items} == {"dir_c"}

    def test_recursive_off_scans_only_root(self):
        with tempfile.TemporaryDirectory() as d:
            # dir_c 是根的直接子目录（应被扫描到）
            # sub/nested.txt 在二级目录（递归关闭时不应出现）
            os.makedirs(os.path.join(d, "sub"))
            os.makedirs(os.path.join(d, "dir_c"))
            with open(os.path.join(d, "sub", "nested.txt"), "w") as f:
                f.write("x")
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["dir_c", "nested.txt"],
                target_paths=[d],
                delete_type="all",
                match_mode="exact",
                ignore_case=True,
                recursive=False,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[],
                result_cb=items.append,
            )
            matched = {os.path.basename(i["path"]) for i in items}
            # 直接子项命中，深层不命中
            assert matched == {"dir_c"}

    def test_exclude_dirs_skips_matches(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["file_a.txt"],
                target_paths=[d],
                delete_type="all",
                match_mode="exact",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=["sub"],
                whitelist=[],
                result_cb=items.append,
            )
            # file_a.txt 在根目录，sub 被排除不影响它；这里验证排除逻辑走通
            assert any(os.path.basename(i["path"]) == "file_a.txt" for i in items)

    def test_whitelist_protects_paths(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []
            eng.scan(
                names=["file_a.txt"],
                target_paths=[d],
                delete_type="all",
                match_mode="exact",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[d],  # 整个根目录白名单 -> 全跳过
                result_cb=items.append,
            )
            assert items == []

    def test_scan_can_be_cancelled_midway(self):
        with tempfile.TemporaryDirectory() as d:
            _build_tree(d)
            eng = ScanEngine()
            items = []

            def on_result(it):
                items.append(it)
                eng.stop()  # 收到第一条结果后立即请求取消

            eng.scan(
                names=["file_a.txt", "file_b.log", "dir_c"],
                target_paths=[d],
                delete_type="all",
                match_mode="exact",
                ignore_case=True,
                recursive=True,
                max_depth=0,
                exclude_dirs=[],
                whitelist=[],
                result_cb=on_result,
            )
            # 取消后应立即停止，至多只产出 1 条
            assert len(items) <= 1


# ==================== 白名单匹配语义 ====================
# 回归背景：白名单原先只做「目录前缀」匹配，用户填文件名（如 数据v1.pdf）
# 会被当成相对路径 → 永远匹配不到 → 文件照删。现支持三种写法。
class TestWhitelistMatching:
    def test_filename_entry_protects_matching_file(self):
        """用户场景：填「数据v1.pdf」必须保护同名文件（含后缀精确匹配）。"""
        assert ScanEngine._in_whitelist(
            r"D:\temp\test_data\数据v1.pdf", ["数据v1.pdf"]) is True
        assert ScanEngine._in_whitelist(
            r"D:\temp\test_data\数据v1.txt", ["数据v1.pdf"]) is False

    def test_filename_match_is_case_insensitive(self):
        assert ScanEngine._in_whitelist(r"D:\t\A.PDF", ["a.pdf"]) is True

    def test_wildcard_entry_matches_extension(self):
        assert ScanEngine._in_whitelist(r"D:\t\数据v1.pdf", ["*.pdf"]) is True
        assert ScanEngine._in_whitelist(r"D:\t\数据v1.txt", ["*.pdf"]) is False
        assert ScanEngine._in_whitelist(r"D:\t\报告v1.docx", ["报告*"]) is True

    def test_directory_entry_protects_subtree(self):
        assert ScanEngine._in_whitelist(r"D:\资料\sub\a.txt", [r"D:\资料"]) is True
        assert ScanEngine._in_whitelist(r"D:\资料", [r"D:\资料"]) is True

    def test_directory_prefix_respects_boundary(self):
        """D:\\资料 不应误保护同前缀的 D:\\资料2。"""
        assert ScanEngine._in_whitelist(r"D:\资料2\a.txt", [r"D:\资料"]) is False

    def test_trailing_separator_and_blank_entries(self):
        assert ScanEngine._in_whitelist(r"D:\资料\a.txt", ["D:\\资料\\"]) is True
        assert ScanEngine._in_whitelist(r"D:\资料\a.txt", ["", "   "]) is False


# ==================== 删除引擎 ====================
class TestDeleteEngine:
    def test_delete_moves_to_trash(self):
        with tempfile.TemporaryDirectory() as d:
            target = os.path.join(d, "del_me.txt")
            with open(target, "w") as f:
                f.write("data")
            eng = DeleteEngine()
            logs = []
            report = eng.delete_items(
                items=[{"path": target, "is_dir": False, "size": 4}],
                whitelist=[],
                progress_cb=lambda dn, tot: None,
                item_cb=logs.append,
                log_cb=lambda lvl, msg: None,
            )
            assert report["success"] == 1
            assert report["failed"] == 0
            assert not os.path.exists(target)  # 已移入回收站

    def test_whitelist_skip(self):
        with tempfile.TemporaryDirectory() as d:
            target = os.path.join(d, "keep.txt")
            with open(target, "w") as f:
                f.write("data")
            eng = DeleteEngine()
            eng.delete_items(
                items=[{"path": target, "is_dir": False, "size": 4}],
                whitelist=[d],
                progress_cb=lambda dn, tot: None,
                item_cb=lambda r: None,
                log_cb=lambda lvl, msg: None,
            )
            assert os.path.exists(target)  # 白名单保护

    def test_whitelist_filename_protects_file(self):
        """白名单填文件名（带后缀）时，删除阶段也必须跳过。

        用户场景：whitelist = ["数据v1.pdf"]，该文件必须完好无损。"""
        d = tempfile.mkdtemp(prefix="fj_wl_test_", dir=os.path.expanduser("~"))
        try:
            target = os.path.join(d, "数据v1.pdf")
            with open(target, "w", encoding="utf-8") as f:
                f.write("data")
            eng = DeleteEngine()
            recs = []
            report = eng.delete_items(
                items=[{"path": target, "is_dir": False, "size": 4}],
                whitelist=["数据v1.pdf"],
                progress_cb=lambda dn, tot: None,
                item_cb=recs.append,
                log_cb=lambda lvl, msg: None,
            )
            assert report["success"] == 0 and report["skipped"] == 1, report
            assert recs and recs[0]["error"] == "白名单保护", recs
            assert os.path.exists(target)
        finally:
            shutil.rmtree(d, ignore_errors=True)

    def test_missing_path_skipped(self):
        with tempfile.TemporaryDirectory() as d:
            gone = os.path.join(d, "ghost.txt")
            eng = DeleteEngine()
            logs = []
            eng.delete_items(
                items=[{"path": gone, "is_dir": False, "size": 0}],
                whitelist=[],
                progress_cb=lambda dn, tot: None,
                item_cb=logs.append,
                log_cb=lambda lvl, msg: None,
            )
            assert logs and logs[0]["result"] == "skipped"

    def test_system_protected_path_skipped(self):
        # 用真实的系统路径（Windows）验证保护兜底，不真正删除
        protected = r"C:\Windows\System32\test_probe_xyz.txt"
        eng = DeleteEngine()
        logs = []
        eng.delete_items(
            items=[{"path": protected, "is_dir": False, "size": 0}],
            whitelist=[],
            progress_cb=lambda dn, tot: None,
            item_cb=logs.append,
            log_cb=lambda lvl, msg: None,
        )
        assert logs and logs[0]["result"] == "skipped"


if __name__ == "__main__":
    sys.exit(pytest.main([__file__, "-v"]))
