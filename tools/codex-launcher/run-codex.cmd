@echo off
rem Reference process-boundary wrapper. Use a unique tag for each invocation.
if "%~3"=="" exit /b 2
set "GH_LANE=%~1"
set "GH_TAG=%~2"
set "GH_PROMPT=%~3"
set "GH_RESULT=C:\gh\%GH_LANE%\last-%GH_TAG%.md"
call codex exec -o "%GH_RESULT%" - < "%GH_PROMPT%"
set "GH_EXIT=%ERRORLEVEL%"
call graphhelm tasks run-ended --lane "%GH_LANE%" --exit %GH_EXIT% --run-id "%GH_TAG%" --result-file "%GH_RESULT%" --token-file "F:\github\GraphHelm\.graphhelm\events.agent.token"
exit /b %GH_EXIT%
