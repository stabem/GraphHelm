import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { FlowRow, JourneyView, LaneRow, LanesView, StepView } from '../types'

// Nothing is hard-coded to a machine: the project is the session's cwd, the binary is found on PATH,
// and the thumbnail helper sits beside this file in the plugin. `ready` fills these before first use.
let EXE = 'graphhelm'
let PROJECT = ''
let TOKEN = ''
let THUMB = ''
const ready = async ($: any) => {
  if (PROJECT && THUMB) return
  const slash = (p: string) => p.split('\\').join('/')
  PROJECT = slash(String(await $.session.cwd()))
  TOKEN = `${PROJECT}/.graphhelm/events.token`
  THUMB = `${slash(String($.plugin.root))}/hooks/thumb.py`
}
const SVG_MAX = 120000
const PREFIX = 'journey-'
const EXP = { w: 1100, h: 688 }
const LIST = 'journeys'
const TOOL = 'mcp__journey-review__show_journey'

const views = atom({ plugin: 'journey-review', key: 'views' } as const, {} as Record<string, JourneyView>)
const flowsAtom = atom({ plugin: 'journey-review', key: 'flows' } as const, [] as FlowRow[])
const flowsErr = atom({ plugin: 'journey-review', key: 'flowsError' } as const, null as string | null)
const selectedAtom = atom({ plugin: 'journey-review', key: 'selected' } as const, {} as Record<string, string>)

// The token is passed as a file path only; its contents are never read here.
const approveArgv = (id: string): string[] => [
  EXE, 'journey', 'approve', id, '--project', PROJECT, '--token-file', TOKEN, '--json',
]
const approveText = (id: string): string =>
  `graphhelm journey approve ${id} --project ${PROJECT} --token-file ${TOKEN}`

const COLOR: Record<string, string> = { pass: 'green', drift: 'yellow', fail: 'red', failed: 'red' }
const HEX: Record<string, string> = { pass: '#2ea043', drift: '#d29922', fail: '#f85149', failed: '#f85149' }
const colorOf = (r: string) => COLOR[r] ?? 'gray'
const hexOf = (r: string) => HEX[r] ?? '#8b949e'
const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')

const REASONS: Record<string, string> = {
  page_error: 'the page showed an error',
  expect_missing: 'an expected element did not appear',
  act_failed: 'an action could not be done',
  not_reached: 'the run never got to this step',
  timeout: 'the step took too long',
  internal: 'the preview tool hit an internal error',
}
const reasonText = (code: string) => REASONS[code] ?? code

const listOf = (yaml: string, key: string): string[] => {
  const m = new RegExp('^' + key + ':\\s*\\[(.*)\\]', 'm').exec(yaml)
  return m ? (m[1] ?? '').split(',').map(x => x.trim()).filter(Boolean) : []
}

const parse = (out: string): any => {
  try { return JSON.parse(out) } catch { return null }
}

// Main path order: first edge's source screen, then each edge's target.
const order = (flow: any): any[] => {
  const screens: any[] = flow?.screens ?? []
  const byId = new Map(screens.map(s => [s.id, s]))
  const edges: any[] = flow?.edges ?? []
  const main: string[] = flow?.paths?.main ?? edges.map(e => e.id)
  const out: any[] = []
  const add = (id: string) => {
    const s = byId.get(id)
    if (s && !out.includes(s)) out.push(s)
  }
  const first = edges.find(e => e.id === main[0])
  if (first) add(first.from)
  for (const eid of main) {
    const e = edges.find(x => x.id === eid)
    if (e) add(e.to)
  }
  for (const s of screens) add(s.id)
  return out
}

const buildView = (id: string, flow: any, kept: any): Partial<JourneyView> => {
  const steps: StepView[] = order(flow).map(s => {
    const run = kept?.screens?.[s.id] ?? {}
    const checks =
      Array.isArray(run.checks) && run.checks.length
        ? run.checks.map((c: any) => ({ role: String(c.role), name: String(c.name), seen: c.seen === true }))
        : (s.expect ?? []).map((c: any) => ({ role: String(c.role), name: String(c.name), seen: null }))
    const edge = (flow?.edges ?? []).find((x: any) => x.to === s.id)
    const action = edge ? (edge.acts ?? []).map((a: any) => a.name).filter(Boolean).join(', ') : undefined
    return {
      id: s.id,
      title: s.title ?? s.id,
      result: run.result ?? 'not run',
      reason: run.reason,
      frame: run.frame === true ? `${PROJECT}/.graphhelm/journey-previews/${id}/frames/${s.id}.png` : undefined,
      action: action || undefined,
      checks,
    }
  })
  const last = order(flow).slice(-1)[0]
  const proves = (last?.expect ?? []).map((c: any) => `${c.role} ${c.name}`).join(', ')
  return {
    title: flow?.title ?? id,
    purpose: flow?.purpose,
    proves: proves ? `"${last?.title ?? 'the last step'}" shows ${proves}` : undefined,
    steps,
    overall: kept?.result ?? kept?.state,
    commit: kept?.commit,
    ranAt: kept?.ranAt,
  }
}

const runPreview = async ($: any, id: string, force: boolean) => {
  await ready($)
  const argv = [EXE, 'journey', 'preview', id, '--project', PROJECT, ...(force ? ['--force'] : ['--read']), '--json']
  const p = await $.process.run(argv, { timeoutMs: 120000 })
  return { exitCode: p.exitCode as number, kept: parse(p.stdout)?.data }
}

const load = async ($: any, id: string, force: boolean) => {
  await ready($)
  const set = (patch: Partial<JourneyView>) =>
    update($, views, (all: Record<string, JourneyView>) => ({
      ...all,
      [id]: { id, title: id, loading: false, steps: [], rejecting: false, note: '', ...all[id], ...patch },
    }))
  await set({ loading: true, error: undefined })
  const f = await $.process.run([EXE, 'journey', 'flows', '--project', PROJECT, '--json'], { timeoutMs: 60000 })
  const flow = (parse(f.stdout)?.data?.flows as any[] | undefined)?.find(x => x.id === id)
  if (!flow) return set({ loading: false, error: `Flow "${id}" not found (flows exit ${f.exitCode}).` })

  let { exitCode, kept } = await runPreview($, id, force)
  // A run is in the background: poll until it ends (at most about three minutes).
  for (let n = 0; kept?.state === 'running' && n < 90; n++) {
    await $.clock.sleep(2000)
    ;({ kept } = await runPreview($, id, false))
  }
  if (!kept) return set({ loading: false, error: `Preview gave no JSON (exit ${exitCode}).` })

  // Nothing kept for the current commit: take the newest kept preview of any commit.
  let older: string | undefined
  let empty = false
  if (kept.state === 'none') {
    let last: any = null
    try {
      last = parse(await $.fs.read(`${PROJECT}/.graphhelm/journey-previews/${id}/state.json`))
    } catch { /* nothing kept at all */ }
    if (last && last.screens && Object.keys(last.screens).length && last.state !== 'running') {
      kept = last
      older = String(last.commit ?? '').slice(0, 8) || undefined
    } else {
      empty = true
    }
  }
  // A fresh run resets the selection to the first step that is not pass.
  await update($, selectedAtom, (all: Record<string, string>) => {
    const { [id]: _drop, ...rest } = all
    return rest
  })
  // The flow file adds who it is for and what it risks (the flows JSON does not carry them).
  let actors: string[] = []
  let risks: string[] = []
  try {
    const yaml = String(await $.fs.read(`${PROJECT}/.graphhelm/journeys/${id}.journey.yaml`))
    actors = listOf(yaml, 'actors')
    risks = listOf(yaml, 'risks')
  } catch { /* no flow file: the block shows what the flows JSON has */ }
  return set({ ...buildView(id, flow, kept), actors, risks, loading: false, older, empty })
}

// Thumbnails are made when a card is drawn, only for what it shows, and cached
// by frame path + mtime + size (module memory: a reload starts it empty).
const cache = new Map<string, { w: number; h: number; b64: string } | null>()
const thumb = async ($: any, path: string, w: number, h: number, q = 70, limit = 90000) => {
  await ready($)
  let mtime = 0
  try { mtime = Number((await $.fs.stat(path))?.mtimeMs ?? 0) } catch { /* no stat: key by path */ }
  const key = `${path}|${mtime}|${w}x${h}|${q}`
  if (cache.has(key)) return cache.get(key) ?? null
  const t = await $.process.run(['python', THUMB, path, String(w), String(h), String(q), String(limit)], { timeoutMs: 30000 })
  const [dims, b64] = String(t.stdout).trim().split(/\r?\n/)
  const [tw, th] = String(dims).split(' ').map(Number)
  const out = t.exitCode === 0 && b64 && tw && th ? { w: tw, h: th, b64 } : null
  cache.set(key, out)
  return out
}

const relTime = (iso?: string): string => {
  const t = iso ? Date.parse(iso) : NaN
  if (!Number.isFinite(t)) return 'time unknown'
  const s = Math.max(0, Math.round((Date.now() - t) / 1000))
  if (s < 90) return 'just now'
  if (s < 5400) return `${Math.round(s / 60)} min ago`
  if (s < 129600) return `${Math.round(s / 3600)} h ago`
  return `${Math.round(s / 86400)} d ago`
}

const placeholderSvg = (note: string) => {
  const { w, h } = EXP
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<rect width="${w}" height="${h}" rx="10" fill="#161b22"/>` +
    `<text x="${w / 2}" y="${h / 2}" text-anchor="middle" font-size="22" fill="#8b949e" font-family="sans-serif">${esc(note)}</text></svg>`
  )
}

// The journey card: header, one large frame of the selected step with Prev/Next/Open original,
// caption, checks, one button per step, and a footer with the buttons.
const draw = async ($: any, e: any, v: JourneyView, id: string) => {
  const { Box, Text, Button, Image, Input, Svg } = $.ui.resolve(e) as any
  const isTerminal = e.surface === 'terminal'
  const patch = (p: Partial<JourneyView>) =>
    update($, views, (all: Record<string, JourneyView>) => ({ ...all, [id]: { ...all[id], ...p } }))

  const approve = async (press: any) => {
    await ready($)
    const r = await $.process.run(approveArgv(id), { timeoutMs: 120000 })
    if (r.exitCode === 0 && parse(r.stdout)?.ok !== false) {
      $.ui.toast(`Approved ${id}.`)
      await load($, id, false)
    } else {
      await $.ui.copy({ text: approveText(id), surface: press?.surface })
      $.ui.toast(`Approve failed (exit ${r.exitCode}); command copied. Run it in a terminal.`)
    }
  }
  const reject = async (press: any, note: string) => {
    await $.ui.copy({ text: `Journey ${id} rejected: ${note}`, surface: press?.surface })
    $.ui.toast('GraphHelm has no reject command; your note is copied. Paste it in the PR or issue.')
    await patch({ rejecting: false, note: '' })
  }

  const steps = v.steps
  const total = steps.length
  const passed = steps.filter(s => s.result === 'pass').length
  const anyFail = steps.some(s => s.result === 'fail' || s.result === 'failed')
  const tally = total === 0 ? 'gray' : anyFail ? 'red' : passed === total ? 'green' : 'yellow'

  // Selection: the atom, else the first step that is not pass, else step 1.
  const chosen = (await read($, selectedAtom))[id]
  const firstOpen = steps.findIndex(s => s.result !== 'pass')
  const selIndex = Math.max(0, steps.findIndex(s => s.id === chosen) >= 0
    ? steps.findIndex(s => s.id === chosen)
    : firstOpen >= 0 ? firstOpen : 0)
  const sel = steps[selIndex]

  // Desktop picture, made lazily for the selected step only: one large JPEG (cached by path + mtime).
  let frameSvg: string | null = null
  if (!isTerminal && sel) {
    const ran = sel.result !== 'not run'
    if (sel.frame) {
      const huge = await thumb($, sel.frame, EXP.w, EXP.h, 80, 125000)
      if (huge) {
        const src = `<svg xmlns="http://www.w3.org/2000/svg" width="${huge.w}" height="${huge.h}" viewBox="0 0 ${huge.w} ${huge.h}"><image href="data:image/jpeg;base64,${huge.b64}" width="${huge.w}" height="${huge.h}"/></svg>`
        if (src.length <= 131072) frameSvg = src
      }
    } else {
      frameSvg = placeholderSvg(ran ? 'No screenshot for this step.' : 'No screenshot. The run stopped before this step.')
    }
  }
  const go = (to: number) => async () => {
    const t = steps[to]
    if (t) await update($, selectedAtom, (all: Record<string, string>) => ({ ...all, [id]: t.id }))
  }
  const original = async () => {
    if (!sel?.frame) return
    // explorer.exe wants backslashes; the process runs hidden.
    await $.process.run(['explorer.exe', sel.frame.split('/').join(String.fromCharCode(92))], { timeoutMs: 15000 })
  }
  const pick = (stepId: string) => () => update($, selectedAtom, (all: Record<string, string>) => ({ ...all, [id]: stepId }))
  const short = (t: string) => (t.length > 18 ? `${t.slice(0, 17)}…` : t)
  const mark = (r: string) => (r === 'pass' ? '✓' : r === 'not run' ? '·' : '✗')

  const checks: any[] = []
  if (sel) {
    if (sel.result === 'not run') {
      checks.push(<Text key="none" dimColor>Checks did not run</Text>)
    } else {
      sel.checks.forEach((c, i) =>
        checks.push(
          <Text key={`c${i}`} color={c.seen === true ? 'green' : c.seen === false ? 'red' : undefined}>
            {c.seen === true ? '✓' : c.seen === false ? '✗' : '·'} {c.role} {c.name}
          </Text>,
        ))
      if (sel.reason && sel.result !== 'pass') {
        checks.push(
          <Box key="why" flexDirection="row" gap={1}>
            <Text color="red">✗ {reasonText(sel.reason)}</Text>
            <Text dimColor>({sel.reason})</Text>
          </Box>,
        )
      }
    }
  }

  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1} gap={1}>
      <Box flexDirection="row" justifyContent="space-between" alignItems="flex-start" gap={2}>
        <Box flexDirection="column" flexShrink={1}>
          <Text bold>{v.title}</Text>
          <Text dimColor>
            {id} on {v.commit ? v.commit.slice(0, 8) : 'no commit'}, {relTime(v.ranAt)}{v.older ? ' (older)' : ''}
          </Text>
        </Box>
        <Text bold color={tally}>{passed} of {total} passed</Text>
      </Box>
      <Box flexDirection="column">
        <Box flexDirection="row" flexWrap="wrap" gap={1}>
          <Text dimColor>Journey</Text>
          <Text>{id.split('-').join(' ')}</Text>
        </Box>
        {v.purpose && (
          <Box flexDirection="row" flexWrap="wrap" gap={1}>
            <Text dimColor>Why</Text>
            <Text>{v.purpose}</Text>
          </Box>
        )}
        {v.actors && v.actors.length > 0 && (
          <Box flexDirection="row" flexWrap="wrap" gap={1}>
            <Text dimColor>For whom</Text>
            <Text>the {v.actors.join(', ')}</Text>
          </Box>
        )}
        {v.proves && (
          <Box flexDirection="row" flexWrap="wrap" gap={1}>
            <Text dimColor>Proves when</Text>
            <Text>{v.proves}</Text>
          </Box>
        )}
        <Box flexDirection="row" flexWrap="wrap" gap={1}>
          <Text dimColor>Steps</Text>
          <Text>{total} {total === 1 ? 'step' : 'steps'}, checked by a headless browser run along the main path{v.risks && v.risks.length ? `; risks: ${v.risks.join(', ')}` : ''}</Text>
        </Box>
      </Box>
      {v.loading && <Text dimColor>Running…</Text>}
      {v.error && <Text color="red">{v.error}</Text>}
      {v.empty && !v.loading && <Text dimColor>No run yet — press Refresh</Text>}
      {!v.empty && sel && (
        <Box flexDirection="column" gap={1}>
          <Box flexDirection="row" flexWrap="wrap" gap={1} alignItems="center">
            <Text bold>{selIndex + 1} / {total}  {sel.title}</Text>
            <Button key="prev" onPress={go(selIndex - 1)}>Prev</Button>
            <Button key="next" onPress={go(selIndex + 1)}>Next</Button>
            <Button key="original" onPress={original}>Open original</Button>
          </Box>
          {isTerminal ? (
            sel.frame ? (
              <Image key={`img-${sel.id}`} source={{ file: sel.frame, format: 'png' }} columns={100} rows={28} alt={sel.title} />
            ) : (
              <Text dimColor>No screenshot. The run stopped before this step.</Text>
            )
          ) : frameSvg ? (
            <Svg source={frameSvg} alt={sel.title} />
          ) : (
            <Text dimColor>frame too big to draw inline: {sel.frame}</Text>
          )}
          <Box flexDirection="row" flexWrap="wrap" gap={1}>
            <Text bold>{selIndex + 1}. {sel.title}</Text>
            {sel.action && <Text dimColor>after you {sel.action}</Text>}
          </Box>
          <Box flexDirection="row" flexWrap="wrap" gap={2}>{checks}</Box>
          <Box flexDirection="row" flexWrap="wrap" gap={1}>
            {steps.map((s, i) => (
              <Button key={`pick-${s.id}`} variant={i === selIndex ? 'primary' : undefined} onPress={pick(s.id)}>
                {i + 1} {short(s.title).padEnd(18, ' ')} <Text color={colorOf(s.result)}>{mark(s.result)}</Text>
              </Button>
            ))}
          </Box>
        </Box>
      )}
      {v.rejecting && (
        <Input
          key="note"
          label="Note: "
          placeholder="why reject?"
          value={v.note}
          onInput={(t: string) => { void patch({ note: t }) }}
          onSubmit={(t: string, p: any) => reject(p, t)}
        />
      )}
      {!isTerminal && (
        <Svg source={'<svg xmlns="http://www.w3.org/2000/svg" width="520" height="1"><rect width="520" height="1" fill="#8b949e" fill-opacity="0.35"/></svg>'} alt="" width={520} height={1} />
      )}
      <Box flexDirection="row" justifyContent="space-between" alignItems="center" gap={2}>
        <Button key="refresh" onPress={() => load($, id, true)}>Run again</Button>
        <Box flexDirection="row" gap={1}>
          <Button key="reject" onPress={() => patch({ rejecting: true })}>Reject</Button>
          <Button key="approve" variant="primary" onPress={approve}>Approve</Button>
        </Box>
      </Box>
    </Box>
  )
}

const viewOf = async ($: any, id: string) => (await read($, views))[id] as JourneyView | undefined

const openFlow = async ($: any, id: string) => {
  await $.ui.open({ id: PREFIX + id, title: `Journey ${id}`, focus: true })
  await load($, id, false)
}

const PY = 'python'
const LANES_PANE = 'lanes'
const LANES_TOOL = 'mcp__journey-review__show_lanes'
const SILENT_MS = 30 * 60 * 1000
const EVERY_MS = 30000

const COLUMNS = [
  { id: 'implement', label: 'Implement' },
  { id: 'review', label: 'Review' },
  { id: 'blocked', label: 'Blocked' },
  { id: 'build', label: 'Build queue' },
  { id: 'silent', label: 'Silent' },
  { id: 'merge', label: 'Merge' },
] as const

const lanesAtom = atom({ plugin: 'journey-review', key: 'lanes' } as const, {
  rows: [], loading: false, collapsed: {},
} as LanesView)

const ms = (iso?: string | null): number => {
  const t = iso ? Date.parse(iso) : NaN
  return Number.isFinite(t) ? t : NaN
}

const span = (from: number, to: number): string => {
  if (!Number.isFinite(from) || !Number.isFinite(to)) return '?'
  const m = Math.max(0, Math.round((to - from) / 60000))
  if (m < 1) return '<1m'
  if (m < 90) return `${m}m`
  if (m < 2880) return `${Math.round(m / 60)}h`
  return `${Math.round(m / 1440)}d`
}

// Same rules as the Studio board: a lane that holds or waits for a build slot is in Build queue;
// a stalled task or a silent lane is Silent; a fix/blocked task is Blocked; then the step.
const toRows = (data: any): LaneRow[] => {
  const now = ms(data?.now) || Date.now()
  const slotLanes = new Set<string>()
  for (const s of data?.slots ?? []) {
    if (s?.holder?.lane) slotLanes.add(String(s.holder.lane))
    for (const w of s?.waiting ?? []) if (w?.lane) slotLanes.add(String(w.lane))
  }
  const stalls = new Set<string>(data?.stalls ?? [])
  const silent = new Set<string>(data?.silent ?? [])
  const rows: LaneRow[] = []
  for (const t of data?.tasks ?? []) {
    if (t.releasedBy || t.step === 'merged') continue
    const lane = String(t.lane ?? '')
    const last = Math.max(ms(t.clock?.lastAt) || 0, ms(t.lastHeartbeatAt) || 0, ms(t.claimedAt) || 0) || NaN
    const since = ms(t.clock?.since) || ms(t.claimedAt)
    const idle = Number.isFinite(last) ? now - last : NaN
    let column: LaneRow['column']
    if (slotLanes.has(lane)) column = 'build'
    else if (stalls.has(t.key) || silent.has(lane)) column = 'silent'
    else if (t.step === 'fix' || t.step === 'critic' || t.blockedBy) column = 'blocked'
    else if (t.step === 'review') column = 'review'
    else if (t.step === 'merge') column = 'merge'
    else column = 'implement'
    rows.push({
      key: String(t.key), issue: Number(t.issue), title: String(t.title ?? t.prTitle ?? ''), step: String(t.step ?? ''),
      lane, pr: t.pr ?? null, inStep: span(since, now), idle: span(last, now),
      isStale: !Number.isFinite(idle) || idle > SILENT_MS, idleMs: Number.isFinite(idle) ? idle : -1, column,
    })
  }
  return rows.sort((a, b) => b.idleMs - a.idleMs)
}

// The CLI has no command that lists the open board, so a small helper reads the token file and
// asks the Runtime. The project gives the token path; the helper finds the Runtime URL and the
// execution in the project's `.graphhelm/team.json`. The token is read inside that child process
// and never printed or stored here.
const fetchBoard = async ($: any): Promise<{ rows?: LaneRow[]; error?: string }> => {
  await ready($)
  const helper = THUMB.replace(/thumb\.py$/, 'lanes.py')
  const r = await $.process.run([PY, '-I', helper, PROJECT, TOKEN], { timeoutMs: 60000 })
  let data: any = null
  try { data = JSON.parse(String(r.stdout).trim().split(/\r?\n/).pop() ?? '') } catch { /* below */ }
  if (r.exitCode !== 0 || !data) return { error: `Lanes failed (exit ${r.exitCode}). Is the Runtime up?` }
  return { rows: toRows(data) }
}

const refresh = async ($: any) => {
  await update($, lanesAtom, (v: LanesView) => ({ ...v, loading: true }))
  const out = await fetchBoard($)
  await update($, lanesAtom, (v: LanesView) => ({
    ...v, loading: false, error: out.error, rows: out.rows ?? v.rows,
  }))
}

let timer: { cancel: () => void } | null = null

const summary = (rows: LaneRow[]): string => {
  const counts = COLUMNS.map(c => `${c.label} ${rows.filter(r => r.column === c.id).length}`).join(', ')
  const red = rows.filter(r => r.isStale).length
  return `${rows.length} open tasks. ${counts}. No activity over 30 min: ${red}.`
}

async function openLanes($: any) {
  const opened = await $.ui.open({ id: LANES_PANE, title: 'Lanes', focus: true })
  await refresh($)
  if (!timer) timer = $.clock.every(EVERY_MS, () => refresh($))
  return opened
}


const lanesStart = async ($: any) => {
  await $.command.register({ name: 'lanes', description: 'Open the GraphHelm Lanes board in a side panel' })
  await $.tool.register({
    name: 'show_lanes',
    description: 'Open the GraphHelm Lanes board (open tasks by column) in a side panel for the owner.',
    inputSchema: { type: 'object', properties: {} },
    isDeferred: false,
  })
}



export const register: Register = on => {
  on('command.run', { command: 'lanes' }, async $ => {
    const opened = await openLanes($)
    return { text: opened?.isPlaced === false ? `Lanes pane not placed: ${(opened as any).reason}` : 'Lanes panel opened.' }
  })

  on('tool.call', { tool: LANES_TOOL }, async $ => {
    await openLanes($)
    const v = await read($, lanesAtom)
    return { result: v.error ? `Lanes panel opened, but: ${v.error}` : `Lanes panel opened. ${summary(v.rows)}` }
  })

  on('ui.render', { component: 'Pane', requestId: LANES_PANE }, async ($, e) => {
    const { Box, Text, Button } = $.ui.resolve(e) as any
    const v = await read($, lanesAtom)
    const toggle = (id: string) => () =>
      update($, lanesAtom, (s: LanesView) => ({ ...s, collapsed: { ...s.collapsed, [id]: !s.collapsed[id] } }))
    return (
      <Box flexDirection="column" paddingX={1} gap={1}>
        <Box flexDirection="row" flexWrap="wrap" gap={1} alignItems="center">
          <Button key="refresh" onPress={() => refresh($)}>Refresh</Button>
          {COLUMNS.map(c => (
            <Text key={`n-${c.id}`} bold>{`${c.label} ${v.rows.filter(r => r.column === c.id).length}`}</Text>
          ))}
        </Box>
        {v.loading && <Text dimColor>Loading…</Text>}
        {v.error && <Text color="red">{v.error}</Text>}
        {COLUMNS.map(c => {
          const list = v.rows.filter(r => r.column === c.id)
          const shut = v.collapsed[c.id] === true
          return (
            <Box key={`g-${c.id}`} flexDirection="column">
              <Button key={`toggle-${c.id}`} onPress={toggle(c.id)}>{`${shut ? '▸' : '▾'} ${c.label} (${list.length})`}</Button>
              {!shut && list.length === 0 && <Text dimColor>  none</Text>}
              {!shut && list.map(r => (
                <Box key={`t-${r.key}`} flexDirection="column" marginLeft={2}>
                  <Text color={r.isStale ? 'red' : undefined} wrap="truncate-end">{`#${r.issue} ${r.title}`}</Text>
                  <Text dimColor={!r.isStale} color={r.isStale ? 'red' : undefined}>
                    {`${r.step} · ${r.lane} · ${r.pr ? `PR ${r.pr}` : 'no PR'} · in step ${r.inStep} · idle ${r.idle}`}
                  </Text>
                </Box>
              ))}
            </Box>
          )
        })}
      </Box>
    )
  })

  on('session.start', async ($, e, next) => {
    await $.command.register({ name: 'journey', description: 'Show a GraphHelm journey inline: /journey <flow-id>' })
    await $.command.register({ name: 'journeys', description: 'List GraphHelm journey flows' })
    await lanesStart($)
    await $.tool.register({
      name: 'show_journey',
      description: 'Show a GraphHelm journey run (frames, checks, Approve/Reject/Run again) inline in the chat for the owner.',
      inputSchema: { type: 'object', properties: { id: { type: 'string', description: 'flow id, e.g. studio-connect' } }, required: ['id'] },
      isDeferred: false,
    })
    return next(e)
  })

  // /journey <id>: load, then CommandOutput draws the card inline.
  on('command.run', { command: 'journey' }, async ($, e) => {
    const id = String((e as any).args ?? '').trim().split(/\s+/)[0]
    if (!id) return { text: 'Usage: /journey <flow-id>' }
    await load($, id, false)
    return { text: `Journey ${id}` }
  })

  on('ui.render', { component: 'CommandOutput', props: { command: 'journey' } }, async ($, e, next) => {
    const id = String((e as any).props?.args ?? '').trim().split(/\s+/)[0]
    const v = id ? await viewOf($, id) : undefined
    if (!v) return next(e)
    return draw($, e, v, id)
  })

  // show_journey(id): the model calls it; its arguments sit flat on the event.
  // The result must be a string; the id rides in its first words for ToolResult.
  on('tool.call', { tool: TOOL }, async ($, e) => {
    const id = String((e as any).id ?? (e as any).input?.id ?? '').trim()
    if (!id) return { deny: 'show_journey needs an id.' }
    await load($, id, false)
    const v = await viewOf($, id)
    const steps = (v?.steps ?? []).map(s => `${s.title}: ${s.result}`).join('; ')
    return { result: `Journey ${id} shown to the owner. Result: ${v?.overall ?? 'unknown'}. ${steps}` }
  })

  on('ui.render', { component: 'ToolResult', props: { tool: TOOL } }, async ($, e, next) => {
    const out = (e as any).props?.output
    const raw = typeof out === 'string' ? out : Array.isArray(out) ? out.map((c: any) => c?.text ?? '').join(' ') : ''
    const id = /^Journey (\S+) shown/.exec(raw)?.[1] ?? ''
    const v = id ? await viewOf($, id) : undefined
    if (!v) return next(e)
    return draw($, e, v, id)
  })

  // A run of calls folds into one ToolGroup line and the group draws no ToolResult:
  // when our tool is in a group, keep the engine's line and draw the cards under it.
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    const mine = ((e as any).props?.calls ?? []).filter((c: any) => c?.tool === TOOL && !c.isRunning && !c.isErrored)
    if (!mine.length) return next(e)
    const { Box } = $.ui.resolve(e) as any
    const cards: any[] = []
    for (const c of mine) {
      const id = String(c.input?.id ?? '').trim()
      const v = id ? await viewOf($, id) : undefined
      if (v) cards.push(await draw($, e, v, id))
    }
    if (!cards.length) return next(e)
    const line = await next(e)
    return <Box flexDirection="column" gap={1}>{line}{cards}</Box>
  })

  on('command.run', { command: 'journeys' }, async $ => {
    await $.ui.open({ id: LIST, title: 'Journeys' })
    await ready($)
    const r = await $.process.run([EXE, 'journey', 'flows', '--project', PROJECT, '--json'], { timeoutMs: 60000 })
    const flows = parse(r.stdout)?.data?.flows as any[] | undefined
    await update($, flowsAtom, () =>
      (flows ?? []).map(f => ({ id: f.id, title: f.title ?? f.id, status: f.status ?? '' })))
    await update($, flowsErr, () => (flows ? null : `flows failed (exit ${r.exitCode})`))
    return { text: 'Journeys pane opened.' }
  })

  // Pane: kept because it is cheap, it draws the same card. /journeys buttons open it.
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    const rid = (e as any).requestId as string
    if (rid !== LIST && !rid?.startsWith(PREFIX)) return next(e)
    const { Box, Text, Button } = $.ui.resolve(e) as any
    if (rid === LIST) {
      const flows = await read($, flowsAtom)
      const err = await read($, flowsErr)
      return (
        <Box flexDirection="column">
          {err && <Text color="red">{err}</Text>}
          {flows.map(f => (
            <Button key={`open-${f.id}`} onPress={() => openFlow($, f.id)}>
              {f.id} - {f.title} ({f.status})
            </Button>
          ))}
        </Box>
      )
    }
    const id = rid.slice(PREFIX.length)
    const v = await viewOf($, id)
    if (!v) return <Text dimColor>Loading…</Text>
    return draw($, e, v, id)
  })
}
