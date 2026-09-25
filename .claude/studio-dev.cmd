@echo off
rem #1058 (Codex on PR #1060): setlocal, so nothing set here leaks into a persistent cmd.exe
rem session and a later launch from another checkout cannot inherit this one's GRAPHHELM_EVENTS.
setlocal
rem Dev-server launcher for the Studio preview. Values here are PATHS and a local URL - the
rem token itself stays in the file graphhelm serve wrote; nothing secret lives in this script.
rem GRAPHHELM_EVENTS defaults to the events dir of the repo this script lives in
rem (a preset value wins, so serving another project stays possible deliberately).
rem GRAPHHELM_PROJECT is always derived from the resolved GRAPHHELM_EVENTS path's
rem repository folder name, never spelled here and never overridable: an override
rem door on a derived value is how it starts disagreeing with the directory it labels.
if not defined GRAPHHELM_EVENTS set "GRAPHHELM_EVENTS=%~dp0..\.graphhelm\events"
set GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
for %%I in ("%GRAPHHELM_EVENTS%\..\..") do set "GRAPHHELM_PROJECT=%%~nxI"
rem Session nonce for the auto-connect endpoint (PR #467): MINTED PER LAUNCH, never a fixed
rem value - this file is committed, and a constant here would be a credential in the repo that
rem any local user could read and exchange for the bearer token while the dev server runs
rem (PR #467 review, P1). The URL echoes to THIS terminal only, the owner's own channel.
for /f %%i in ('powershell -NoProfile -Command "[Guid]::NewGuid().ToString()"') do set GRAPHHELM_STUDIO_SESSION_NONCE=%%i
echo Studio auto-connect: http://127.0.0.1:5183/?session=%GRAPHHELM_STUDIO_SESSION_NONCE%
cd /d "%~dp0..\apps\studio"
npm run dev -- --port 5183 --strictPort
