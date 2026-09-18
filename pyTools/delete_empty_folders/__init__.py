import os
import sys


def delete_empty_folders(path):
    """
    删除指定路径下所有的空文件夹。

    :param path: 要处理的根目录路径
    """
    # 检查提供的路径是否真实存在且为目录
    if not os.path.exists(path):
        print(f"错误：路径 {path} 不存在。")
        return

    if not os.path.isdir(path):
        print(f"错误：{path} 不是目录。")
        return

    # 遍历目录并尝试删除空文件夹
    for root, dirs, files in os.walk(path, topdown=False):
        for dir in dirs:
            dir_path = os.path.join(root, dir)
            try:
                # 如果文件夹为空，则删除
                if not os.listdir(dir_path):  # 判断文件夹是否为空
                    os.rmdir(dir_path)
                    print(f"已删除空文件夹：{dir_path}")
            except Exception as e:
                print(f"跳过删除空文件夹 {dir_path} 时出错: {e}")


def main():
    """百宝箱入口函数"""
    target_directory = input("请输入要处理的目录路径：")
    delete_empty_folders(target_directory)


if __name__ == "__main__":
    main()
