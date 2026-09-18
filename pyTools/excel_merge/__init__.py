"""
Excel 合并工具包
根据指定列匹配合并两个 Excel 文件
"""

from .excel_merge_core import (
    ExcelMergeInput,
    merge_excel_files,
    get_columns_from_file,
    get_sheets_from_file
)
from .excel_merge_dialog import (
    ExcelMergeDialog,
    show_merge_dialog
)

__all__ = [
    'ExcelMergeInput',
    'merge_excel_files',
    'get_columns_from_file',
    'get_sheets_from_file',
    'ExcelMergeDialog',
    'show_merge_dialog',
]
