"""
Excel 合并工具独立运行脚本
直接运行此文件即可启动 Excel 合并工具
"""
import sys
import os


def main():
    """百宝箱入口函数"""
    from PyQt5.QtWidgets import QApplication
    from PyQt5.QtGui import QFont
    
    # 脚本所在目录添加到 path，供 excel_merge_dialog 导入依赖
    script_dir = os.path.dirname(os.path.abspath(__file__))
    if script_dir not in sys.path:
        sys.path.insert(0, script_dir)
    
    from excel_merge_dialog import show_merge_dialog
    
    app = QApplication(sys.argv)
    app.setFont(QFont("Microsoft YaHei", 10))
    
    dialog = show_merge_dialog()
    
    sys.exit(0)


if __name__ == "__main__":
    main()
