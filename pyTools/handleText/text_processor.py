"""
示例工具 - 文本处理器
演示文本处理工具的集成
"""
import sys


def text_processor():
    """文本处理功能"""
    from PyQt5.QtWidgets import (QDialog, QVBoxLayout, QHBoxLayout, 
                                 QTextEdit, QPushButton, QLabel, QComboBox, QApplication)
    from PyQt5.QtCore import Qt
    import sys
    
    class TextProcessorDialog(QDialog):
        def __init__(self):
            super().__init__()
            self.setWindowTitle("文本处理器")
            self.setFixedSize(700, 600)  # 设置固定大小
            self.init_ui()
            # 设置窗口在屏幕中间
            screen = QApplication.primaryScreen()
            screen_geometry = screen.geometry()
            self.move(
                (screen_geometry.width() - self.width()) // 2,
                (screen_geometry.height() - self.height()) // 2
            )
            
        def init_ui(self):
            layout = QVBoxLayout(self)
            layout.setSpacing(10)
            layout.setContentsMargins(15, 15, 15, 15)
            
            # 输入区域
            input_label = QLabel("输入文本:")
            input_label.setStyleSheet("font-weight: bold; font-size: 12px;")
            layout.addWidget(input_label)
            
            self.input_text = QTextEdit()
            self.input_text.setPlaceholderText("在此输入要处理的文本...")
            self.input_text.setMaximumHeight(150)
            layout.addWidget(self.input_text)
            
            # 操作按钮
            btn_layout = QHBoxLayout()
            
            self.operation_combo = QComboBox()
            self.operation_combo.addItems([
                "转为大写",
                "转为小写",
                "去除首尾空格",
                "去除所有空格",
                "统计字符数",
                "统计行数",
                "反转文本"
            ])
            self.operation_combo.setStyleSheet("""
                QComboBox {
                    padding: 5px;
                    border: 1px solid #ccc;
                    border-radius: 3px;
                    min-width: 150px;
                }
            """)
            btn_layout.addWidget(self.operation_combo)
            
            process_btn = QPushButton("执行")
            process_btn.setStyleSheet("""
                QPushButton {
                    background-color: #4A90E2;
                    color: white;
                    border: none;
                    padding: 8px 20px;
                    border-radius: 5px;
                    font-weight: bold;
                }
                QPushButton:hover {
                    background-color: #357ABD;
                }
            """)
            process_btn.clicked.connect(self.process_text)
            btn_layout.addWidget(process_btn)
            
            layout.addLayout(btn_layout)
            
            # 输出区域
            output_label = QLabel("处理结果:")
            output_label.setStyleSheet("font-weight: bold; font-size: 12px;")
            layout.addWidget(output_label)
            
            self.output_text = QTextEdit()
            self.output_text.setReadOnly(True)
            self.output_text.setStyleSheet("background-color: #F9F9F9;")
            layout.addWidget(self.output_text)
            
        def process_text(self):
            """处理文本"""
            text = self.input_text.toPlainText()
            operation = self.operation_combo.currentText()
            
            if not text:
                self.output_text.setText("请输入文本！")
                return
                
            try:
                if operation == "转为大写":
                    result = text.upper()
                elif operation == "转为小写":
                    result = text.lower()
                elif operation == "去除首尾空格":
                    result = text.strip()
                elif operation == "去除所有空格":
                    result = text.replace(" ", "").replace("\t", "").replace("\n", "")
                elif operation == "统计字符数":
                    non_whitespace = len(text.replace(' ', '').replace('\n', '').replace('\t', ''))
                    result = f"总字符数: {len(text)}\n非空白字符: {non_whitespace}"
                elif operation == "统计行数":
                    lines = text.split('\n')
                    result = f"总行数: {len(lines)}\n非空行数: {len([l for l in lines if l.strip()])}"
                elif operation == "反转文本":
                    result = text[::-1]
                else:
                    result = "未知操作"
                    
                self.output_text.setText(result)
                
            except Exception as e:
                self.output_text.setText(f"错误: {str(e)}")
    
    # 创建独立的QApplication实例
    app = QApplication(sys.argv)
    dialog = TextProcessorDialog()
    dialog.exec_()


def main():
    """主函数 - 百宝箱调用入口"""
    # 使用 QApplication.instance() 获取现有实例，如果不存在则创建
    from PyQt5.QtWidgets import QApplication
    app = QApplication.instance()
    if app is None:
        app = QApplication(sys.argv)
    
    try:
        text_processor()
    except Exception as e:
        print(f"错误: {e}")
        import traceback
        traceback.print_exc()


if __name__ == "__main__":
    main()
