chcp 65001 >nul
@echo off
setlocal enabledelayedexpansion

:: ========== 权限检查 ==========
net session >nul 2>&1
if %errorlevel% neq 0 (
    echo 请以管理员身份运行此脚本。
    pause
    exit /b
)

:: ========== 清理临时文件 ==========
echo 正在清理系统临时文件...
if exist "%SystemRoot%\Temp" (
    del /q /f /s "%SystemRoot%\Temp\*" >nul 2>&1
    for /d %%p in ("%SystemRoot%\Temp\*") do rd /s /q "%%p" >nul 2>&1
)

echo 正在清理用户临时文件...
if exist "%TEMP%" (
    del /q /f /s "%TEMP%\*" >nul 2>&1
    for /d %%p in ("%TEMP%\*") do rd /s /q "%%p" >nul 2>&1
)



echo 清理完成。
pause
exit /b
