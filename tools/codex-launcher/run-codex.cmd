@echo off
rem Arguments are parsed once by Python; never CALL a command assembled from lane values.
python "%~dp0run_codex.py" %*
exit /b %ERRORLEVEL%
