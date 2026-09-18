import pandas as pd
import os
from tkinter import Tk, Button, Label, Entry, filedialog, messagebox, Checkbutton, BooleanVar


def merge_excel_files(file_paths, output_file, keep_headers=True):
    """
    合并多个Excel文件到一个Excel文件中。

    :param file_paths: 需要合并的Excel文件路径列表
    :param output_file: 合并后的输出文件路径
    :param keep_headers: 是否保留每个文件的首行标题，默认为True
    """
    dataframes = []  # 用于存储所有读取的DataFrame

    for i, file_path in enumerate(file_paths):
        df = pd.read_excel(file_path)

        # 如果不是第一个文件且需要跳过标题行
        if not keep_headers and i > 0:
            df = df.iloc[1:]

        dataframes.append(df)  # 将处理后的DataFrame添加到列表中

    # 一次性合并所有DataFrame
    merged_df = pd.concat(dataframes, ignore_index=True)
    merged_df.to_excel(output_file, index=False)
    messagebox.showinfo("完成", f"文件已合并并保存为 {output_file}")


def select_files():
    """打开文件选择对话框，选择需要合并的Excel文件"""
    file_paths = filedialog.askopenfilenames(
        title="选择需要合并的Excel文件",
        filetypes=[("Excel files", "*.xlsx")]
    )
    if file_paths:
        file_entry.delete(0, "end")
        file_entry.insert(0, "; ".join(file_paths))


def start_merge():
    """开始合并操作"""
    file_paths = file_entry.get().split("; ")
    if not file_paths:
        messagebox.showwarning("警告", "未选择任何文件。")
        return

    keep_headers = keep_headers_var.get()
    first_file_path = file_paths[0]
    output_directory = os.path.dirname(first_file_path)
    output_file = os.path.join(output_directory, "合并后的文件.xlsx")

    merge_excel_files(file_paths, output_file, keep_headers)


def main():
    """百宝箱入口函数 - GUI界面"""
    # 创建主窗口
    root = Tk()
    root.title("Excel 文件合并工具")
    
    # 文件选择部分
    file_label = Label(root, text="选择文件:")
    file_label.grid(row=0, column=0, padx=5, pady=5)
    
    file_entry = Entry(root, width=50)
    file_entry.grid(row=0, column=1, padx=5, pady=5)
    
    file_button = Button(root, text="浏览", command=select_files)
    file_button.grid(row=0, column=2, padx=5, pady=5)
    
    # 是否保留标题行
    keep_headers_var = BooleanVar(value=True)
    keep_headers_check = Checkbutton(root, text="保留首行标题", variable=keep_headers_var)
    keep_headers_check.grid(row=1, column=1, padx=5, pady=5, sticky="w")
    
    # 开始合并按钮
    merge_button = Button(root, text="开始合并", command=start_merge)
    merge_button.grid(row=2, column=1, padx=5, pady=10)
    
    # 运行主循环
    root.mainloop()


if __name__ == "__main__":
    main()
