import os
import sys
from PyQt5.QtWidgets import (QApplication, QDialog, QVBoxLayout, QHBoxLayout, 
                             QLabel, QLineEdit, QPushButton, QFileDialog,
                             QMessageBox, QDoubleSpinBox, QListWidget)

def get_folder_size(folder_path, skip_paths):
    """
    计算指定文件夹的大小。

    :param folder_path: 要计算大小的文件夹路径
    :param skip_paths: 要跳过的路径列表
    :return: 文件夹的大小（以GB为单位）
    """
    if folder_path in skip_paths:
        return 0

    total_size = 0
    for dirpath, dirnames, filenames in os.walk(folder_path):
        # 跳过指定的路径
        dirnames[:] = [d for d in dirnames if os.path.join(dirpath, d) not in skip_paths]
        for f in filenames:
            fp = os.path.join(dirpath, f)
            # 跳过符号链接
            if not os.path.islink(fp):
                total_size += os.path.getsize(fp)
    return total_size / (1024 ** 3)  # 转换为GB

def print_folder_sizes(root_path, size_threshold, skip_paths):
    """
    打印指定路径下所有文件夹的大小。

    :param root_path: 要处理的根目录路径
    :param size_threshold: 文件夹大小阈值（以GB为单位）
    :param skip_paths: 要跳过的路径列表
    """
    # 检查提供的路径是否真实存在且为目录
    if not os.path.exists(root_path):
        print(f"错误：路径 {root_path} 不存在。")
        return

    if not os.path.isdir(root_path):
        print(f"错误：{root_path} 不是目录。")
        return

    # 遍历目录并打印每个文件夹的大小
    for root, dirs, _ in os.walk(root_path):
        # 跳过指定的路径
        dirs[:] = [d for d in dirs if os.path.join(root, d) not in skip_paths]
        for dir in dirs:
            dir_path = os.path.join(root, dir)
            size = get_folder_size(dir_path, skip_paths)
            if size > size_threshold:  # 使用用户输入的阈值
                print(f"文件夹 {dir_path} 的大小: {size:.2f} GB")  # 格式化为两位小数的GB

def main():
    """百宝箱入口函数 - 提供GUI界面"""
    
    class FolderSizeCheckerDialog(QDialog):
        def __init__(self):
            super().__init__()
            self.setWindowTitle("📁 文件夹大小检查器")
            self.setFixedSize(700, 600)
            self.skip_paths = []
            self.init_ui()
            
        def init_ui(self):
            layout = QVBoxLayout(self)
            layout.setSpacing(15)
            layout.setContentsMargins(20, 20, 20, 20)
            
            # 标题
            title = QLabel("📁 文件夹大小检查器")
            title.setStyleSheet("font-size: 20px; font-weight: bold; color: #2C3E50;")
            layout.addWidget(title)
            
            # 目标目录选择
            dir_layout = QHBoxLayout()
            dir_label = QLabel("目标目录:")
            dir_label.setStyleSheet("font-weight: bold;")
            dir_layout.addWidget(dir_label)
            
            self.dir_input = QLineEdit()
            self.dir_input.setPlaceholderText("选择或输入要检查的目录...")
            self.dir_input.setStyleSheet("padding: 5px;")
            dir_layout.addWidget(self.dir_input)
            
            browse_btn = QPushButton("浏览...")
            browse_btn.setStyleSheet("""
                QPushButton {
                    background-color: #4A90E2;
                    color: white;
                    border: none;
                    padding: 5px 15px;
                    border-radius: 3px;
                }
                QPushButton:hover {
                    background-color: #357ABD;
                }
            """)
            browse_btn.clicked.connect(self.browse_directory)
            dir_layout.addWidget(browse_btn)
            layout.addLayout(dir_layout)
            
            # 大小阈值
            threshold_layout = QHBoxLayout()
            threshold_label = QLabel("大小阈值(GB):")
            threshold_label.setStyleSheet("font-weight: bold;")
            threshold_layout.addWidget(threshold_label)
            
            self.threshold_spin = QDoubleSpinBox()
            self.threshold_spin.setRange(0, 1000)
            self.threshold_spin.setValue(1.0)
            self.threshold_spin.setDecimals(2)
            self.threshold_spin.setStyleSheet("padding: 5px;")
            threshold_layout.addWidget(self.threshold_spin)
            layout.addLayout(threshold_layout)
            
            # 跳过路径列表
            skip_label = QLabel("跳过路径列表:")
            skip_label.setStyleSheet("font-weight: bold;")
            layout.addWidget(skip_label)
            
            self.skip_list = QListWidget()
            self.skip_list.setMaximumHeight(100)
            self.skip_list.setStyleSheet("border: 1px solid #ccc; border-radius: 3px;")
            layout.addWidget(self.skip_list)
            
            skip_btn_layout = QHBoxLayout()
            add_skip_btn = QPushButton("+ 添加跳过路径")
            add_skip_btn.setStyleSheet("""
                QPushButton {
                    background-color: #27AE60;
                    color: white;
                    border: none;
                    padding: 5px 10px;
                    border-radius: 3px;
                }
                QPushButton:hover {
                    background-color: #229954;
                }
            """)
            add_skip_btn.clicked.connect(self.add_skip_path)
            skip_btn_layout.addWidget(add_skip_btn)
            
            remove_skip_btn = QPushButton("- 移除选中")
            remove_skip_btn.setStyleSheet("""
                QPushButton {
                    background-color: #E74C3C;
                    color: white;
                    border: none;
                    padding: 5px 10px;
                    border-radius: 3px;
                }
                QPushButton:hover {
                    background-color: #C0392B;
                }
            """)
            remove_skip_btn.clicked.connect(self.remove_skip_path)
            skip_btn_layout.addWidget(remove_skip_btn)
            layout.addLayout(skip_btn_layout)
            
            # 执行按钮
            check_btn = QPushButton("🔍 开始检查")
            check_btn.setStyleSheet("""
                QPushButton {
                    background-color: #4A90E2;
                    color: white;
                    border: none;
                    padding: 12px;
                    border-radius: 5px;
                    font-size: 14px;
                    font-weight: bold;
                }
                QPushButton:hover {
                    background-color: #357ABD;
                }
                QPushButton:pressed {
                    background-color: #2A5F9E;
                }
            """)
            check_btn.clicked.connect(self.check_folders)
            layout.addWidget(check_btn)
            
            # 结果显示区域
            result_label = QLabel("检查结果:")
            result_label.setStyleSheet("font-weight: bold; margin-top: 10px;")
            layout.addWidget(result_label)
            
            self.result_text = QListWidget()
            self.result_text.setStyleSheet("border: 1px solid #ccc; border-radius: 3px; background-color: #F9F9F9;")
            layout.addWidget(self.result_text)
            
        def browse_directory(self):
            """浏览选择目录"""
            folder = QFileDialog.getExistingDirectory(self, "选择要检查的目录")
            if folder:
                self.dir_input.setText(folder)
                
        def add_skip_path(self):
            """添加跳过路径"""
            folder = QFileDialog.getExistingDirectory(self, "选择要跳过的目录")
            if folder and folder not in self.skip_paths:
                self.skip_paths.append(folder)
                self.skip_list.addItem(folder)
                
        def remove_skip_path(self):
            """移除选中的跳过路径"""
            current_item = self.skip_list.currentItem()
            if current_item:
                path = current_item.text()
                self.skip_paths.remove(path)
                self.skip_list.takeItem(self.skip_list.row(current_item))
                
        def check_folders(self):
            """执行文件夹检查"""
            target_dir = self.dir_input.text().strip()
            if not target_dir:
                QMessageBox.warning(self, "警告", "请选择目标目录！")
                return
                
            if not os.path.exists(target_dir):
                QMessageBox.warning(self, "错误", f"目录不存在: {target_dir}")
                return
                
            threshold = self.threshold_spin.value()
            self.result_text.clear()
            
            try:
                found_large = False
                for root, dirs, _ in os.walk(target_dir):
                    # 跳过指定的路径
                    dirs[:] = [d for d in dirs if os.path.join(root, d) not in self.skip_paths]
                    
                    for dir_name in dirs:
                        dir_path = os.path.join(root, dir_name)
                        size = get_folder_size(dir_path, self.skip_paths)
                        
                        if size > threshold:
                            found_large = True
                            item_text = f"📂 {dir_path}\n   大小: {size:.2f} GB"
                            self.result_text.addItem(item_text)
                            
                if not found_large:
                    self.result_text.addItem(f"✅ 未发现超过 {threshold} GB 的文件夹")
                    
                QMessageBox.information(self, "完成", f"检查完成！共找到 {self.result_text.count()} 个结果")
                    
            except Exception as e:
                QMessageBox.critical(self, "错误", f"检查过程中出错:\n{str(e)}")
    
    app = QApplication.instance()
    if app is None:
        app = QApplication(sys.argv)
    
    dialog = FolderSizeCheckerDialog()
    dialog.exec_()

if __name__ == "__main__":
    main()