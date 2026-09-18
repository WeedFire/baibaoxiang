"""
Excel 合并工具 - 根据指定列匹配合并两个 Excel 文件
支持不同列名匹配、选择列复制、left/inner 合并模式
"""

import pandas as pd
import os
from typing import List, Optional
from pydantic import BaseModel, Field


class ExcelMergeInput(BaseModel):
    """Excel 合并输入参数"""
    file_a: str = Field(..., description="A 表文件路径")
    file_b: str = Field(..., description="B 表文件路径")
    key_column_a: str = Field(..., description="A 表匹配列名")
    key_column_b: str = Field(..., description="B 表匹配列名")
    columns_to_copy: List[str] = Field(..., description="需要从 A 复制到 B 的列名列表")
    sheet_a: str = Field(default="Sheet1", description="A 表 Sheet 名称")
    sheet_b: str = Field(default="Sheet1", description="B 表 Sheet 名称")
    merge_mode: str = Field(default="left", description="合并方式: left 或 inner")
    output_file: Optional[str] = Field(default=None, description="输出文件路径")


def merge_excel_files(params: ExcelMergeInput) -> str:
    """
    合并两个 Excel 文件：根据指定列名匹配行，并将 A 表指定列数据填充到 B 表。

    该工具支持：
    - 两个文件使用不同的列名进行匹配（如 A 表用 '订单号'，B 表用 '订单ID'）
    - 选择多个列从 A 表复制到 B 表
    - 自动在 B 表中创建不存在的目标列
    - left 合并保留 B 表所有行，inner 合并仅保留匹配行

    Args:
        params (ExcelMergeInput): 输入参数

    Returns:
        str: JSON 格式的合并结果
    """
    import json
    
    try:
        # 验证文件存在
        if not os.path.exists(params.file_a):
            return json.dumps({
                "success": False,
                "error": f"A 表文件不存在: {params.file_a}"
            }, ensure_ascii=False)
        
        if not os.path.exists(params.file_b):
            return json.dumps({
                "success": False,
                "error": f"B 表文件不存在: {params.file_b}"
            }, ensure_ascii=False)
        
        # 读取 A 表
        try:
            df_a = pd.read_excel(params.file_a, sheet_name=params.sheet_a)
        except ValueError:
            return json.dumps({
                "success": False,
                "error": f"A 表中不存在 Sheet: {params.sheet_a}"
            }, ensure_ascii=False)
        except Exception as e:
            return json.dumps({
                "success": False,
                "error": f"读取 A 表失败: {str(e)}"
            }, ensure_ascii=False)
        
        # 读取 B 表
        try:
            df_b = pd.read_excel(params.file_b, sheet_name=params.sheet_b)
        except ValueError:
            return json.dumps({
                "success": False,
                "error": f"B 表中不存在 Sheet: {params.sheet_b}"
            }, ensure_ascii=False)
        except Exception as e:
            return json.dumps({
                "success": False,
                "error": f"读取 B 表失败: {str(e)}"
            }, ensure_ascii=False)
        
        # 验证匹配列存在
        if params.key_column_a not in df_a.columns:
            available_cols = ", ".join(df_a.columns.tolist())
            return json.dumps({
                "success": False,
                "error": f"A 表中未找到列 '{params.key_column_a}'，可用列: {available_cols}"
            }, ensure_ascii=False)
        
        if params.key_column_b not in df_b.columns:
            available_cols = ", ".join(df_b.columns.tolist())
            return json.dumps({
                "success": False,
                "error": f"B 表中未找到列 '{params.key_column_b}'，可用列: {available_cols}"
            }, ensure_ascii=False)
        
        # 验证要复制的列存在
        columns_to_copy = []
        columns_not_found = []
        for col in params.columns_to_copy:
            if col in df_a.columns:
                columns_to_copy.append(col)
            else:
                columns_not_found.append(col)
        
        # 记录实际复制的列
        columns_added = []
        for col in columns_to_copy:
            if col not in df_b.columns:
                columns_added.append(col)
        
        # 确定合并模式
        how = params.merge_mode if params.merge_mode in ['left', 'inner', 'right', 'outer'] else 'left'
        
        # 执行合并：使用 A 表的匹配列作为左键，B 表的匹配列作为右键
        # 先用 A 表的匹配列和要复制的列创建临时 DataFrame
        merge_df = df_a[[params.key_column_a] + columns_to_copy].copy()
        merge_df = merge_df.rename(columns={params.key_column_a: '__merge_key__'})
        df_b_orig = df_b.copy()  # 保存原始 B 表用于统计
        df_b = df_b.rename(columns={params.key_column_b: '__merge_key__'})
        
        # 合并
        merged_df = df_b.merge(merge_df, on='__merge_key__', how=how)
        merged_df = merged_df.rename(columns={'__merge_key__': params.key_column_b})
        
        # 统计匹配行数
        rows_total_b = len(df_b_orig)
        if how == 'inner':
            # inner 模式：所有行都是匹配的
            rows_matched = len(merged_df)
        else:
            # left 模式：计算有匹配值的行数
            # 检查第一个复制列是否有值来判断是否匹配
            if columns_to_copy:
                rows_matched = merged_df[columns_to_copy[0]].notna().sum()
            else:
                rows_matched = len(merged_df)
        
        # 确定输出文件路径
        if params.output_file:
            output_file = params.output_file
        else:
            output_dir = os.path.dirname(params.file_b) or "."
            base_name = os.path.splitext(os.path.basename(params.file_b))[0]
            output_file = os.path.join(output_dir, f"{base_name}_合并结果.xlsx")
        
        # 保存结果
        merged_df.to_excel(output_file, index=False)
        
        return json.dumps({
            "success": True,
            "file_a": params.file_a,
            "file_b": params.file_b,
            "rows_matched": rows_matched,
            "rows_total_b": rows_total_b,
            "columns_copied": columns_to_copy,
            "columns_added": columns_added,
            "output_file": output_file,
            "message": f"成功合并 {rows_matched} 行数据，输出文件: {output_file}"
        }, ensure_ascii=False)
        
    except Exception as e:
        import json
        return json.dumps({
            "success": False,
            "error": str(e)
        }, ensure_ascii=False)


def get_columns_from_file(file_path: str, sheet_name: str = "Sheet1") -> List[str]:
    """获取 Excel 文件的列名列表"""
    try:
        if not os.path.exists(file_path):
            return []
        df = pd.read_excel(file_path, sheet_name=sheet_name)
        return df.columns.tolist()
    except:
        return []


def get_sheets_from_file(file_path: str) -> List[str]:
    """获取 Excel 文件的所有 Sheet 名称"""
    try:
        if not os.path.exists(file_path):
            return []
        xl_file = pd.ExcelFile(file_path)
        return xl_file.sheet_names
    except:
        return []


if __name__ == "__main__":
    # 测试
    print("Excel 合并工具模块")
