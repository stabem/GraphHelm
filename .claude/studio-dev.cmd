@echo off
rem Dev-server launcher for the Studio preview. Values here are PATHS and a local URL - the
rem token itself stays in the file graphhelm serve wrote; nothing secret lives in this script.
set GRAPHHELM_EVENTS=F:\github\Dale\dale-api-base\.graphhelm\events
set GRAPHHELM_RUNTIME_URL=http://127.0.0.1:8791
set GRAPHHELM_PROJECT=dale-api-base
rem Session nonce for the auto-connect endpoint (PR #467): MINTED PER LAUNCH, never a fixed
rem value - this file is committed, and a constant here would be a credential in the repo that
rem any local user could read and exchange for the bearer token while the dev server runs
rem (PR #467 review, P1). The URL echoes to THIS terminal only, the owner's own channel.
for /f %%i in ('powershell -NoProfile -Command "[Guid]::NewGuid().ToString()"') do set GRAPHHELM_STUDIO_SESSION_NONCE=%%i
echo Studio auto-connect: http://127.0.0.1:5183/?session=%GRAPHHELM_STUDIO_SESSION_NONCE%
cd /d "%~dp0..\apps\studio"
npm run dev -- --port 5183 --strictPort
