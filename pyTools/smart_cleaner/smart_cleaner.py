# smart_cleaner.py
import os
import sys
import json
import shutil
import hashlib
import threading
import tkinter as tk
from tkinter import ttk, filedialog, messagebox, scrolledtext
from datetime import datetime, timedelta
try:
    from send2trash import send2trash
    SEND2TRASH_AVAILABLE = True
except ImportError:
    send2trash = None  # 定义变量以避免NameError
    SEND2TRASH_AVAILABLE = False
    print("警告: send2trash 未安装，将使用普通删除方式")

# ==================== 配置与规则 ====================
CONFIG = {
    "temp_extensions": {".tmp", ".temp", ".bak", ".log", ".dmp", "~", ".chk"},
    "cache_dirs": {"Cache", "cache", "Temp", "temp", "Temporary"},
    "min_size_mb": 50,  # 大文件阈值（MB）
    "old_file_days": 180,  # 旧文件阈值（天）
    "system_protected": [
        os.environ.get("WINDIR", "C:\\Windows"),
        os.environ.get("PROGRAMFILES", "C:\\Program Files"),
        os.environ.get("PROGRAMFILES(X86)", "C:\\Program Files (x86)"),
        os.environ.get("APPDATA", ""),
        os.path.join(os.environ.get("USERPROFILE", ""), "Desktop")
    ]
}


# ==================== 核心扫描引擎 ====================
class CleanerEngine:
    def __init__(self):
        self.results = []  # 存储 (路径, 类型, 大小, 修改时间)
        self.stop_flag = False

    def _is_protected(self, path):
        """安全防护：禁止扫描系统关键目录"""
        abs_path = os.path.abspath(path).lower()
        for protected in CONFIG["system_protected"]:
            if protected and abs_path.startswith(os.path.abspath(protected).lower()):
                return True
        return False

    def _get_file_type(self, file_path, stats):
        """智能分类文件"""
        ext = os.path.splitext(file_path)[1].lower()
        name = os.path.basename(file_path)

        # 临时文件
        if ext in CONFIG["temp_extensions"] or name.startswith("~"):
            return "临时文件"
        # 大文件
        if stats.st_size > CONFIG["min_size_mb"] * 1024 * 1024:
            return "大文件"
        # 旧文件
        if (datetime.now() - datetime.fromtimestamp(stats.st_mtime)).days > CONFIG["old_file_days"]:
            return "长期未使用"
        # 空文件
        if stats.st_size == 0:
            return "空文件"
        # 缓存目录中的文件
        if any(cache_dir in file_path.split(os.sep) for cache_dir in CONFIG["cache_dirs"]):
            return "缓存文件"
        return None

    def scan_directory(self, target_path, progress_callback):
        """深度扫描目标目录"""
        if self._is_protected(target_path):
            raise PermissionError(f"禁止扫描系统保护目录: {target_path}")

        total = sum([len(files) for _, _, files in os.walk(target_path)])
        count = 0
        self.results.clear()

        for root, dirs, files in os.walk(target_path):
            if self.stop_flag:
                return False
            # 跳过系统隐藏目录
            dirs[:] = [d for d in dirs if not d.startswith('.')]

            for file in files:
                if self.stop_flag:
                    return False
                file_path = os.path.join(root, file)
                try:
                    if os.path.islink(file_path):  # 跳过符号链接
                        continue
                    stats = os.stat(file_path)
                    file_type = self._get_file_type(file_path, stats)
                    if file_type:
                        size_mb = stats.st_size / (1024 * 1024)
                        mod_time = datetime.fromtimestamp(stats.st_mtime).strftime("%Y-%m-%d %H:%M")
                        self.results.append((file_path, file_type, size_mb, mod_time))
                except (OSError, PermissionError):
                    continue
                count += 1
                if count % 10 == 0:
                    progress_callback(count / total * 100)
        return True

    def stop_scan(self):
        self.stop_flag = True


# ==================== GUI主界面 ====================
class SmartCleanerApp:
    def __init__(self, root):
        self.root = root
        self.root.title("智能磁盘清理助手 v1.0")
        self.root.geometry("900x600")
        self.root.minsize(800, 500)
        self.engine = CleanerEngine()
        self.selected_items = set()
        self.setup_ui()
        self.scan_thread = None

    def setup_ui(self):
        # 顶部控制区
        control_frame = ttk.Frame(self.root, padding="10")
        control_frame.pack(fill=tk.X)

        ttk.Label(control_frame, text="扫描路径:").pack(side=tk.LEFT, padx=(0, 5))
        self.path_var = tk.StringVar(value=os.path.expanduser("~\\Downloads"))
        ttk.Entry(control_frame, textvariable=self.path_var, width=50).pack(side=tk.LEFT, padx=5)
        ttk.Button(control_frame, text="浏览...", command=self.browse_path).pack(side=tk.LEFT)
        self.scan_btn = ttk.Button(control_frame, text="开始扫描", command=self.start_scan)
        self.scan_btn.pack(side=tk.LEFT, padx=10)
        self.stop_btn = ttk.Button(control_frame, text="停止", command=self.stop_scan, state=tk.DISABLED)
        self.stop_btn.pack(side=tk.LEFT)

        # 进度条
        self.progress = ttk.Progressbar(self.root, mode='determinate')
        self.progress.pack(fill=tk.X, padx=10, pady=5)

        # 结果表格（带勾选框）
        table_frame = ttk.Frame(self.root)
        table_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)

        columns = ("选择", "路径", "类型", "大小(MB)", "修改时间")
        self.tree = ttk.Treeview(table_frame, columns=columns, show="headings", selectmode="extended")
        
        # 设置列标题和宽度
        self.tree.heading("选择", text="☑", anchor=tk.CENTER)
        self.tree.column("选择", width=50, anchor=tk.CENTER)
        
        self.tree.heading("路径", text="路径", anchor=tk.W, command=lambda: self.sort_by_column("路径", False))
        self.tree.column("路径", width=300, anchor=tk.W)
        
        self.tree.heading("类型", text="类型", anchor=tk.CENTER, command=lambda: self.sort_by_column("类型", False))
        self.tree.column("类型", width=100, anchor=tk.CENTER)
        
        self.tree.heading("大小(MB)", text="大小(MB)", anchor=tk.E, command=lambda: self.sort_by_column("大小(MB)", False))
        self.tree.column("大小(MB)", width=100, anchor=tk.E)
        
        self.tree.heading("修改时间", text="修改时间", anchor=tk.CENTER, command=lambda: self.sort_by_column("修改时间", False))
        self.tree.column("修改时间", width=150, anchor=tk.CENTER)
        
        scrollbar = ttk.Scrollbar(table_frame, orient=tk.VERTICAL, command=self.tree.yview)
        self.tree.configure(yscrollcommand=scrollbar.set)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        
        # 绑定点击事件处理勾选
        self.tree.bind("<Button-1>", self.on_tree_click)

        # 底部操作区
        bottom_frame = ttk.Frame(self.root, padding="10")
        bottom_frame.pack(fill=tk.X)

        self.status_var = tk.StringVar(value="就绪")
        ttk.Label(bottom_frame, textvariable=self.status_var, foreground="gray").pack(side=tk.LEFT)
        ttk.Button(bottom_frame, text="全选", command=self.select_all).pack(side=tk.RIGHT, padx=5)
        ttk.Button(bottom_frame, text="反选", command=self.toggle_select).pack(side=tk.RIGHT, padx=5)
        ttk.Button(bottom_frame, text="清空选择", command=self.clear_selection).pack(side=tk.RIGHT, padx=5)
        self.delete_btn = ttk.Button(bottom_frame, text="安全删除选中项", command=self.delete_selected,
                                     state=tk.DISABLED)
        self.delete_btn.pack(side=tk.RIGHT, padx=5)

        # 初始化排序状态
        self.sort_column = None
        self.sort_reverse = False

    def browse_path(self):
        path = filedialog.askdirectory(initialdir=self.path_var.get())
        if path:
            self.path_var.set(path)

    def start_scan(self):
        path = self.path_var.get().strip()
        if not path or not os.path.isdir(path):
            messagebox.showerror("错误", "请选择有效的目录路径！")
            return

        # 二次确认系统目录风险
        if any(path.lower().startswith(p.lower()) for p in CONFIG["system_protected"] if p):
            if not messagebox.askyesno("高风险警告",
                                       "您选择的路径包含系统关键目录！\n误删可能导致系统不稳定。\n确认继续扫描？"):
                return

        self.scan_btn.config(state=tk.DISABLED)
        self.stop_btn.config(state=tk.NORMAL)
        self.delete_btn.config(state=tk.DISABLED)
        self.tree.delete(*self.tree.get_children())
        self.status_var.set("扫描中... 请耐心等待")
        self.progress["value"] = 0

        def scan_worker():
            try:
                success = self.engine.scan_directory(path, self.update_progress)
                if success and not self.engine.stop_flag:
                    self.root.after(0, self.display_results)
                elif self.engine.stop_flag:
                    self.root.after(0, lambda: self.status_var.set("扫描已中止"))
            except Exception as e:
                self.root.after(0, lambda: messagebox.showerror("扫描错误", f"发生错误:\n{str(e)}"))
            finally:
                self.root.after(0, self.scan_complete)

        self.engine.stop_flag = False
        self.scan_thread = threading.Thread(target=scan_worker, daemon=True)
        self.scan_thread.start()

    def update_progress(self, value):
        self.progress["value"] = value

    def scan_complete(self):
        self.scan_btn.config(state=tk.NORMAL)
        self.stop_btn.config(state=tk.DISABLED)
        if not self.engine.stop_flag and self.engine.results:
            self.delete_btn.config(state=tk.NORMAL)
            self.status_var.set(f"扫描完成！发现 {len(self.engine.results)} 个可清理项")

    def stop_scan(self):
        self.engine.stop_scan()
        self.status_var.set("正在中止扫描...")

    def display_results(self):
        for item in self.engine.results:
            iid = self.tree.insert("", tk.END, values=(
                "☐",  # 勾选框初始状态
                item[0],  # 路径
                item[1],  # 类型
                f"{item[2]:.2f}",  # 大小
                item[3]  # 修改时间
            ))
            # 根据类型着色（可选）
            if item[1] == "大文件":
                self.tree.item(iid, tags=("large",))
            elif item[1] == "长期未使用":
                self.tree.item(iid, tags=("old",))
        self.tree.tag_configure("large", background="#fff3cd")
        self.tree.tag_configure("old", background="#e2f0fb")
    
    def sort_by_column(self, col, reverse):
        """按列排序"""
        # 获取所有行的数据
        items = [(self.tree.set(k, col), k) for k in self.tree.get_children('')]
        
        # 根据列类型进行排序
        if col == "大小(MB)":
            # 数值排序
            items.sort(key=lambda x: float(x[0]), reverse=reverse)
        elif col == "路径":
            # 字符串排序
            items.sort(key=lambda x: x[0].lower(), reverse=reverse)
        else:
            # 其他列字符串排序
            items.sort(key=lambda x: x[0], reverse=reverse)
        
        # 重新排列行
        for index, (val, k) in enumerate(items):
            self.tree.move(k, '', index)
        
        # 更新排序状态
        self.sort_column = col
        self.sort_reverse = not reverse
        
        # 更新列标题显示排序方向
        for c in self.tree["columns"]:
            current_text = self.tree.heading(c)["text"]
            # 移除旧的排序箭头
            if current_text.endswith(" ▲") or current_text.endswith(" ▼"):
                current_text = current_text[:-2]
            
            # 添加新的排序箭头
            if c == col:
                arrow = " ▲" if not reverse else " ▼"
                self.tree.heading(c, text=current_text + arrow)
            else:
                self.tree.heading(c, text=current_text)

    def on_tree_click(self, event):
        """处理树形视图点击事件，实现勾选框功能"""
        region = self.tree.identify_region(event.x, event.y)
        column = self.tree.identify_column(event.x)
        item = self.tree.identify_row(event.y)
        
        if region == "cell" and column == "#1" and item:  # 点击的是第一列（勾选列）
            current_values = list(self.tree.item(item)["values"])
            # 切换勾选状态
            if current_values[0] == "☑":
                current_values[0] = "☐"
            else:
                current_values[0] = "☑"
            self.tree.item(item, values=current_values)
            self.update_selected_items()
            return "break"  # 阻止默认行为
    
    def update_selected_items(self):
        """更新选中项集合"""
        self.selected_items = set()
        for item in self.tree.get_children():
            values = self.tree.item(item)["values"]
            if values[0] == "☑":  # 已勾选
                self.selected_items.add(values[1])  # 文件路径在第二列
        
        count = len(self.selected_items)
        if count > 0:
            self.status_var.set(f"已选中 {count} 项")
            self.delete_btn.config(state=tk.NORMAL)
        else:
            self.status_var.set("就绪")
            self.delete_btn.config(state=tk.DISABLED)

    def select_all(self):
        """全选所有项"""
        for item in self.tree.get_children():
            values = list(self.tree.item(item)["values"])
            values[0] = "☑"
            self.tree.item(item, values=values)
        self.update_selected_items()

    def toggle_select(self):
        """反选所有项"""
        for item in self.tree.get_children():
            values = list(self.tree.item(item)["values"])
            values[0] = "☐" if values[0] == "☑" else "☑"
            self.tree.item(item, values=values)
        self.update_selected_items()
    
    def clear_selection(self):
        """清空所有选择"""
        for item in self.tree.get_children():
            values = list(self.tree.item(item)["values"])
            values[0] = "☐"
            self.tree.item(item, values=values)
        self.update_selected_items()

    def delete_selected(self):
        if not self.selected_items:
            messagebox.showinfo("提示", "请先选择要删除的文件")
            return

        # 二次确认 + 安全提示
        delete_method = "移至回收站" if SEND2TRASH_AVAILABLE else "永久删除"
        confirm = messagebox.askyesno(
            "删除确认",
            f"即将{delete_method} {len(self.selected_items)} 个文件\n"
            "⚠️ 重要：部分文件可能关联应用程序，删除前请确认！\n\n"
            "是否继续？"
        )
        if not confirm:
            return

        success_count = 0
        fail_list = []

        for file_path in list(self.selected_items):  # 使用副本避免迭代时修改
            try:
                if os.path.exists(file_path):
                    if SEND2TRASH_AVAILABLE:
                        send2trash(file_path)  # 安全删除：移至回收站
                    else:
                        if os.path.isfile(file_path):
                            os.remove(file_path)  # 普通删除
                        elif os.path.isdir(file_path):
                            shutil.rmtree(file_path)  # 删除目录
                    success_count += 1
                    # 从树中移除已删除的项
                    for item in self.tree.get_children():
                        values = self.tree.item(item)["values"]
                        if values[1] == file_path:  # 路径在第2列
                            self.tree.delete(item)
                            break
            except Exception as e:
                fail_list.append(f"{file_path}: {str(e)}")

        # 更新选中项
        self.update_selected_items()
        
        # 结果反馈
        msg = f"✅ 成功删除 {success_count} 个文件（已{delete_method}）"
        if fail_list:
            msg += f"\n\n❌ 失败 {len(fail_list)} 项:\n" + "\n".join(fail_list[:5])
            if len(fail_list) > 5:
                msg += f"\n...（共{len(fail_list)}项失败）"

        messagebox.showinfo("清理结果", msg)


# ==================== 程序入口 ====================
def main():
    """百宝箱入口函数"""
    # 提示send2trash安装状态
    if not SEND2TRASH_AVAILABLE:
        print("警告: send2trash 未安装，将使用普通删除方式（文件不会移至回收站）")
        print("如需安全删除功能，请执行：pip install send2trash")
        print()

    root = tk.Tk()
    app = SmartCleanerApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()