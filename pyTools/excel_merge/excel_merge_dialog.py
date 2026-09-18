"""
Excel 合并工具 - PyQt5 用户界面
根据指定列匹配合并两个 Excel 文件
"""

import os
import json
from PyQt5.QtWidgets import (
    QDialog, QVBoxLayout, QHBoxLayout, QLabel, QPushButton,
    QComboBox, QListWidget, QListWidgetItem, QCheckBox,
    QGroupBox, QFrame, QScrollArea, QWidget, QMessageBox,
    QProgressBar, QTextEdit
)
from PyQt5.QtCore import Qt, QThread, pyqtSignal
from PyQt5.QtGui import QFont

# 直接导入（支持作为独立脚本运行）
from excel_merge_core import (
    ExcelMergeInput, merge_excel_files, 
    get_columns_from_file, get_sheets_from_file
)


class MergeWorker(QThread):
    """后台合并线程"""
    finished = pyqtSignal(str)
    error = pyqtSignal(str)
    
    def __init__(self, params):
        super().__init__()
        self.params = params
    
    def run(self):
        try:
            result = merge_excel_files(self.params)
            self.finished.emit(result)
        except Exception as e:
            self.error.emit(str(e))


class ExcelMergeDialog(QDialog):
    """Excel 合并对话框"""
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("📊 Excel 匹配合并工具")
        self.setMinimumSize(900, 700)
        self.setModal(True)
        self.setWindowFlags(self.windowFlags() & ~Qt.WindowContextHelpButtonHint)
        
        self.file_a_columns = []
        self.file_b_columns = []
        self.selected_columns = []
        self.worker = None
        
        self._init_ui()
        self._apply_styles()
    
    def _init_ui(self):
        """初始化 UI"""
        main_layout = QVBoxLayout(self)
        main_layout.setSpacing(15)
        main_layout.setContentsMargins(20, 20, 20, 20)
        
        # 标题
        title = QLabel("📊 Excel 匹配合并工具")
        title.setStyleSheet("font-size: 22px; font-weight: bold; color: #2C3E50;")
        title.setAlignment(Qt.AlignCenter)
        main_layout.addWidget(title)
        
        # 说明
        desc = QLabel("根据指定列名匹配两个 Excel 文件，将 A 表数据填充到 B 表")
        desc.setStyleSheet("font-size: 13px; color: #7F8C8D;")
        desc.setAlignment(Qt.AlignCenter)
        main_layout.addWidget(desc)
        
        main_layout.addSpacing(10)
        
        # 文件选择区域
        file_group = self._create_file_selection_group()
        main_layout.addWidget(file_group)
        
        # Sheet 选择区域
        sheet_group = self._create_sheet_selection_group()
        main_layout.addWidget(sheet_group)
        
        # 列选择区域
        column_group = self._create_column_selection_group()
        main_layout.addWidget(column_group)
        
        # 合并选项
        option_group = self._create_option_group()
        main_layout.addWidget(option_group)
        
        # 进度/结果区域
        self.result_edit = QTextEdit()
        self.result_edit.setMaximumHeight(100)
        self.result_edit.setReadOnly(True)
        self.result_edit.setPlaceholderText("合并结果将显示在这里...")
        main_layout.addWidget(self.result_edit)
        
        # 按钮区域
        btn_layout = QHBoxLayout()
        btn_layout.addStretch()
        
        self.merge_btn = QPushButton("🚀 开始合并")
        self.merge_btn.clicked.connect(self._start_merge)
        self.merge_btn.setStyleSheet("""
            QPushButton {
                background-color: #27AE60;
                color: white;
                border: none;
                border-radius: 8px;
                padding: 12px 30px;
                font-weight: bold;
                font-size: 14px;
            }
            QPushButton:hover {
                background-color: #2ECC71;
            }
            QPushButton:disabled {
                background-color: #95A5A6;
            }
        """)
        btn_layout.addWidget(self.merge_btn)
        
        self.close_btn = QPushButton("关闭")
        self.close_btn.clicked.connect(self.accept)
        self.close_btn.setStyleSheet("""
            QPushButton {
                background-color: #95A5A6;
                color: white;
                border: none;
                border-radius: 8px;
                padding: 12px 30px;
                font-weight: bold;
                font-size: 14px;
            }
            QPushButton:hover {
                background-color: #7F8C8D;
            }
        """)
        btn_layout.addWidget(self.close_btn)
        
        main_layout.addLayout(btn_layout)
    
    def _create_file_selection_group(self):
        """创建文件选择区域"""
        group = QGroupBox("📁 文件选择")
        group.setStyleSheet("""
            QGroupBox {
                font-weight: bold;
                font-size: 14px;
                color: #2C3E50;
                border: 2px solid #E8E8E8;
                border-radius: 8px;
                margin-top: 10px;
                padding-top: 10px;
            }
            QGroupBox::title {
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 5px;
            }
        """)
        layout = QVBoxLayout(group)
        layout.setSpacing(12)
        
        # A 表选择
        a_layout = QHBoxLayout()
        a_layout.addWidget(QLabel("A 表文件:"))
        self.file_a_path = QLabel("未选择文件")
        self.file_a_path.setStyleSheet("color: #7F8C8D;")
        self.file_a_path.setMinimumWidth(300)
        a_layout.addWidget(self.file_a_path)
        a_layout.addStretch()
        
        self.btn_select_a = QPushButton("选择 A 表")
        self.btn_select_a.clicked.connect(lambda: self._select_file('a'))
        self.btn_select_a.setStyleSheet(self._btn_style("#3498DB"))
        a_layout.addWidget(self.btn_select_a)
        layout.addLayout(a_layout)
        
        # B 表选择
        b_layout = QHBoxLayout()
        b_layout.addWidget(QLabel("B 表文件:"))
        self.file_b_path = QLabel("未选择文件")
        self.file_b_path.setStyleSheet("color: #7F8C8D;")
        self.file_b_path.setMinimumWidth(300)
        b_layout.addWidget(self.file_b_path)
        b_layout.addStretch()
        
        self.btn_select_b = QPushButton("选择 B 表")
        self.btn_select_b.clicked.connect(lambda: self._select_file('b'))
        self.btn_select_b.setStyleSheet(self._btn_style("#E74C3C"))
        b_layout.addWidget(self.btn_select_b)
        layout.addLayout(b_layout)
        
        return group
    
    def _create_sheet_selection_group(self):
        """创建 Sheet 选择区域"""
        group = QGroupBox("📋 Sheet 选择")
        group.setStyleSheet("""
            QGroupBox {
                font-weight: bold;
                font-size: 14px;
                color: #2C3E50;
                border: 2px solid #E8E8E8;
                border-radius: 8px;
                margin-top: 10px;
                padding-top: 10px;
            }
            QGroupBox::title {
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 5px;
            }
        """)
        layout = QHBoxLayout(group)
        layout.setSpacing(20)
        
        # A 表 Sheet
        layout.addWidget(QLabel("A 表 Sheet:"))
        self.sheet_a_combo = QComboBox()
        self.sheet_a_combo.setMinimumWidth(150)
        self.sheet_a_combo.addItem("Sheet1")
        self.sheet_a_combo.currentTextChanged.connect(self._on_sheet_a_changed)
        layout.addWidget(self.sheet_a_combo)
        
        layout.addSpacing(20)
        
        # B 表 Sheet
        layout.addWidget(QLabel("B 表 Sheet:"))
        self.sheet_b_combo = QComboBox()
        self.sheet_b_combo.setMinimumWidth(150)
        self.sheet_b_combo.addItem("Sheet1")
        self.sheet_b_combo.currentTextChanged.connect(self._on_sheet_b_changed)
        layout.addWidget(self.sheet_b_combo)
        
        layout.addStretch()
        
        return group
    
    def _create_column_selection_group(self):
        """创建列选择区域"""
        group = QGroupBox("🔑 匹配列与复制列设置")
        group.setStyleSheet("""
            QGroupBox {
                font-weight: bold;
                font-size: 14px;
                color: #2C3E50;
                border: 2px solid #E8E8E8;
                border-radius: 8px;
                margin-top: 10px;
                padding-top: 10px;
            }
            QGroupBox::title {
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 5px;
            }
        """)
        layout = QHBoxLayout(group)
        layout.setSpacing(15)
        
        # 匹配列设置
        match_panel = QWidget()
        match_layout = QVBoxLayout(match_panel)
        match_layout.setContentsMargins(0, 0, 0, 0)
        match_layout.setSpacing(8)
        
        match_title = QLabel("匹配键设置")
        match_title.setStyleSheet("font-weight: bold; color: #667eea;")
        match_layout.addWidget(match_title)
        
        match_content = QWidget()
        match_content_layout = QHBoxLayout(match_content)
        match_content_layout.setContentsMargins(0, 0, 0, 0)
        match_content_layout.setSpacing(10)
        
        match_content_layout.addWidget(QLabel("A 表匹配列:"))
        self.key_a_combo = QComboBox()
        self.key_a_combo.setMinimumWidth(120)
        self.key_a_combo.setEnabled(False)
        self.key_a_combo.currentTextChanged.connect(self._validate_merge)
        match_content_layout.addWidget(self.key_a_combo)
        
        match_content_layout.addWidget(QLabel("= B 表匹配列:"))
        self.key_b_combo = QComboBox()
        self.key_b_combo.setMinimumWidth(120)
        self.key_b_combo.setEnabled(False)
        self.key_b_combo.currentTextChanged.connect(self._validate_merge)
        match_content_layout.addWidget(self.key_b_combo)
        
        match_layout.addWidget(match_content)
        layout.addWidget(match_panel, 1)
        
        # 分隔线
        sep = QFrame()
        sep.setFrameShape(QFrame.VLine)
        sep.setStyleSheet("background-color: #E8E8E8;")
        layout.addWidget(sep)
        
        # 复制列选择
        copy_panel = QWidget()
        copy_layout = QVBoxLayout(copy_panel)
        copy_layout.setContentsMargins(0, 0, 0, 0)
        copy_layout.setSpacing(8)
        
        copy_title = QLabel("从 A 表复制到 B 表的列")
        copy_title.setStyleSheet("font-weight: bold; color: #27AE60;")
        copy_layout.addWidget(copy_title)
        
        self.column_list = QListWidget()
        self.column_list.setMaximumHeight(80)
        self.column_list.setSelectionMode(QListWidget.MultiSelection)
        self.column_list.itemSelectionChanged.connect(self._on_column_selection_changed)
        copy_layout.addWidget(self.column_list)
        
        layout.addWidget(copy_panel, 1)
        
        return group
    
    def _create_option_group(self):
        """创建合并选项区域"""
        group = QGroupBox("⚙️ 合并选项")
        group.setStyleSheet("""
            QGroupBox {
                font-weight: bold;
                font-size: 14px;
                color: #2C3E50;
                border: 2px solid #E8E8E8;
                border-radius: 8px;
                margin-top: 10px;
                padding-top: 10px;
            }
            QGroupBox::title {
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 5px;
            }
        """)
        layout = QHBoxLayout(group)
        layout.setSpacing(20)
        
        # 合并模式
        layout.addWidget(QLabel("合并模式:"))
        self.merge_mode_combo = QComboBox()
        self.merge_mode_combo.addItems(["left (保留 B 表所有行)", "inner (仅保留匹配行)"])
        self.merge_mode_combo.setMinimumWidth(200)
        layout.addWidget(self.merge_mode_combo)
        
        layout.addSpacing(20)
        
        # 输出文件
        layout.addWidget(QLabel("输出文件:"))
        self.output_path = QLabel("(默认: B表同目录，文件名后加_合并结果)")
        self.output_path.setStyleSheet("color: #7F8C8D;")
        self.output_path.setMinimumWidth(250)
        layout.addWidget(self.output_path)
        
        btn_set_output = QPushButton("设置输出路径")
        btn_set_output.clicked.connect(self._set_output_path)
        btn_set_output.setStyleSheet(self._btn_style("#9B59B6"))
        layout.addWidget(btn_set_output)
        
        layout.addStretch()
        
        return group
    
    def _btn_style(self, color):
        """生成按钮样式"""
        return f"""
            QPushButton {{
                background-color: {color};
                color: white;
                border: none;
                border-radius: 6px;
                padding: 8px 16px;
                font-weight: bold;
                font-size: 12px;
            }}
            QPushButton:hover {{
                opacity: 0.9;
            }}
            QPushButton:disabled {{
                background-color: #BDC3C7;
            }}
        """
    
    def _apply_styles(self):
        """应用全局样式"""
        self.setStyleSheet("""
            QDialog {
                background-color: #F8F9FA;
            }
            QLabel {
                font-size: 13px;
            }
            QComboBox {
                padding: 6px 10px;
                border: 2px solid #E8E8E8;
                border-radius: 6px;
                font-size: 13px;
                background-color: white;
            }
            QComboBox:hover {
                border-color: #3498DB;
            }
            QListWidget {
                border: 2px solid #E8E8E8;
                border-radius: 6px;
                padding: 5px;
                background-color: white;
            }
            QListWidget::item {
                padding: 5px;
            }
            QListWidget::item:selected {
                background-color: #E8F4FD;
            }
            QTextEdit {
                border: 2px solid #E8E8E8;
                border-radius: 6px;
                padding: 10px;
                background-color: white;
                font-family: 'Consolas', 'Microsoft YaHei';
                font-size: 12px;
            }
        """)
    
    def _select_file(self, which):
        """选择文件"""
        from PyQt5.QtWidgets import QFileDialog
        
        file_path, _ = QFileDialog.getOpenFileName(
            self, 
            f"选择 {'A' if which == 'a' else 'B'} 表 Excel 文件",
            "",
            "Excel 文件 (*.xlsx *.xls);;所有文件 (*.*)"
        )
        
        if file_path:
            if which == 'a':
                self.file_a_path.setText(file_path)
                self._load_file_a_info(file_path)
            else:
                self.file_b_path.setText(file_path)
                self._load_file_b_info(file_path)
    
    def _load_file_a_info(self, file_path):
        """加载 A 表信息"""
        # 加载 Sheet
        self.sheet_a_combo.clear()
        sheets = get_sheets_from_file(file_path)
        if sheets:
            self.sheet_a_combo.addItems(sheets)
        else:
            self.sheet_a_combo.addItem("Sheet1")
        
        # 加载列
        self._on_sheet_a_changed()
    
    def _load_file_b_info(self, file_path):
        """加载 B 表信息"""
        # 加载 Sheet
        self.sheet_b_combo.clear()
        sheets = get_sheets_from_file(file_path)
        if sheets:
            self.sheet_b_combo.addItems(sheets)
        else:
            self.sheet_b_combo.addItem("Sheet1")
        
        # 加载列
        self._on_sheet_b_changed()
    
    def _on_sheet_a_changed(self):
        """A 表 Sheet 改变"""
        file_path = self.file_a_path.text()
        if file_path and os.path.exists(file_path):
            sheet_name = self.sheet_a_combo.currentText()
            columns = get_columns_from_file(file_path, sheet_name)
            self.file_a_columns = columns
            
            # 更新匹配列下拉框
            self.key_a_combo.clear()
            self.key_a_combo.addItems(columns)
            self.key_a_combo.setEnabled(len(columns) > 0)
            
            # 更新复制列列表
            self.column_list.clear()
            for col in columns:
                self.column_list.addItem(col)
    
    def _on_sheet_b_changed(self):
        """B 表 Sheet 改变"""
        file_path = self.file_b_path.text()
        if file_path and os.path.exists(file_path):
            sheet_name = self.sheet_b_combo.currentText()
            columns = get_columns_from_file(file_path, sheet_name)
            self.file_b_columns = columns
            
            # 更新匹配列下拉框
            self.key_b_combo.clear()
            self.key_b_combo.addItems(columns)
            self.key_b_combo.setEnabled(len(columns) > 0)
    
    def _on_column_selection_changed(self):
        """列选择改变"""
        self.selected_columns = [item.text() for item in self.column_list.selectedItems()]
    
    def _set_output_path(self):
        """设置输出路径"""
        from PyQt5.QtWidgets import QFileDialog
        
        file_path, _ = QFileDialog.getSaveFileName(
            self,
            "设置输出文件路径",
            "",
            "Excel 文件 (*.xlsx);;所有文件 (*.*)"
        )
        
        if file_path:
            if not file_path.endswith('.xlsx'):
                file_path += '.xlsx'
            self.output_path.setText(file_path)
    
    def _validate_merge(self):
        """验证合并参数"""
        can_merge = (
            self.file_a_path.text() and os.path.exists(self.file_a_path.text()) and
            self.file_b_path.text() and os.path.exists(self.file_b_path.text()) and
            self.key_a_combo.currentText() and
            self.key_b_combo.currentText() and
            len(self.selected_columns) > 0
        )
        self.merge_btn.setEnabled(can_merge)
    
    def _start_merge(self):
        """开始合并"""
        # 获取参数
        params = ExcelMergeInput(
            file_a=self.file_a_path.text(),
            file_b=self.file_b_path.text(),
            key_column_a=self.key_a_combo.currentText(),
            key_column_b=self.key_b_combo.currentText(),
            columns_to_copy=self.selected_columns,
            sheet_a=self.sheet_a_combo.currentText(),
            sheet_b=self.sheet_b_combo.currentText(),
            merge_mode="left" if "left" in self.merge_mode_combo.currentText() else "inner",
            output_file=self.output_path.text() if self.output_path.text().endswith('.xlsx') else None
        )
        
        # 禁用按钮
        self.merge_btn.setEnabled(False)
        self.result_edit.clear()
        self.result_edit.append("正在合并，请稍候...")
        
        # 启动后台线程
        self.worker = MergeWorker(params)
        self.worker.finished.connect(self._on_merge_finished)
        self.worker.error.connect(self._on_merge_error)
        self.worker.start()
    
    def _on_merge_finished(self, result):
        """合并完成"""
        self.merge_btn.setEnabled(True)
        
        try:
            data = json.loads(result)
            if data.get("success"):
                output_file = data.get("output_file", "")
                self.result_edit.append(f"\n✅ {data.get('message', '合并成功')}")
                self.result_edit.append(f"\n📊 统计信息:")
                self.result_edit.append(f"   - 匹配行数: {data.get('rows_matched', 0)}")
                self.result_edit.append(f"   - B 表总行数: {data.get('rows_total_b', 0)}")
                self.result_edit.append(f"   - 复制列数: {len(data.get('columns_copied', []))}")
                self.result_edit.append(f"   - 新增列数: {len(data.get('columns_added', []))}")
                
                # 打开文件夹
                self._open_output_folder(output_file)
            else:
                self.result_edit.append(f"\n❌ 合并失败: {data.get('error', '未知错误')}")
        except:
            self.result_edit.append(result)
    
    def _on_merge_error(self, error):
        """合并出错"""
        self.merge_btn.setEnabled(True)
        self.result_edit.append(f"\n❌ 错误: {error}")
    
    def _open_output_folder(self, file_path):
        """打开输出文件夹"""
        if file_path and os.path.exists(file_path):
            folder = os.path.dirname(file_path)
            os.startfile(folder)
    
    def get_tool_config(self):
        """获取工具配置（用于集成到百宝箱）"""
        return {
            "id": "excel_merge",
            "type": "python",
            "name": "Excel匹配合并",
            "description": "根据指定列匹配合并两个Excel文件",
            "module": "com.weed.baibaoxiang.excel_merge.excel_merge_dialog",
            "function": "show_merge_dialog",
            "color": "#27AE60"
        }


def show_merge_dialog():
    """显示合并对话框（独立运行入口）"""
    import sys
    from PyQt5.QtWidgets import QApplication
    
    app = QApplication(sys.argv)
    app.setFont(QFont("Microsoft YaHei", 10))
    
    dialog = ExcelMergeDialog()
    dialog.exec_()
    
    return dialog


if __name__ == "__main__":
    show_merge_dialog()
