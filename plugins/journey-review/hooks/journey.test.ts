import { test, expect, mock } from 'claude-code/testing'

const FLOWS: any = {
  ok: true,
  data: {
    flows: [
      {
        id: 'demo', title: 'Demo flow', status: 'approved', purpose: 'The owner finishes the demo.',
        edges: [{ id: 'e1', from: 'a', to: 'b', acts: [] }],
        paths: { main: ['e1'] },
        screens: [
          { id: 'a', title: 'Start screen', expect: [{ role: 'exit', name: '0' }] },
          { id: 'b', title: 'End screen', expect: [{ role: 'text', name: 'Done' }] },
        ],
      },
    ],
  },
}
const KEPT = {
  commit: 'abcdef0123456789',
  state: 'ready', result: 'pass',
  screens: {
    a: { result: 'pass', frame: true, checks: [{ role: 'exit', name: '0', seen: true }] },
    b: { result: 'drift', reason: 'page_error', frame: false, checks: [{ role: 'text', name: 'Done', seen: false }] },
  },
  edges: {},
}
const NONE = { commit: 'ffff0000ffff0000', state: 'none', screens: {}, edges: {} }

const opened: any[] = []
const wire = (on: any, preview: () => any, kept: any, seen: string[][]) => {
  on('ui.open', async (_$: any, e: any) => { opened.push(e); return { value: { isPlaced: true } } })
  // Any absolute directory will do; the plugin's own root is one on every platform.
  on('session.cwd', async (p: any) => ({ value: p.plugin.root }))
  on('ui.copy', async () => ({ value: undefined }))
  on('ui.toast', async () => ({ value: undefined }))
  on('fs.read', async (_$: any, e: any) => {
    if (String(e.path).endsWith('.journey.yaml')) return { value: 'actors: [owner]\nrisks: [authentication, security]\n' }
    if (kept === null) return { value: '' }
    return { value: JSON.stringify(kept) }
  })
  on('process.run', async (_$: any, e: any) => {
    const argv = e.argv as string[]
    seen.push(argv)
    let out: any
    if (argv.some(a => a.endsWith('thumb.py'))) {
      const big = argv.includes('1100') ? 96000 : argv.includes('520') ? 22000 : 1650
      return { value: { exitCode: 0, stdout: '420 262\n' + 'A'.repeat(big) + '\n', stderr: '' } }
    }
    if (argv[0] === 'explorer.exe') return { value: { exitCode: 0, stdout: '', stderr: '' } }
    if (argv.includes('flows')) out = FLOWS
    else if (argv.includes('approve')) out = { ok: true }
    else out = { ok: true, data: preview() }
    return { value: { exitCode: 0, stdout: JSON.stringify(out), stderr: '' } }
  })
}

const text = async (ui: any, re: RegExp) => ui.find({ type: 'Text', text: re })

test('inline command output: cards on terminal and desktop, newest kept preview shown', async ($, on) => {
  const seen: string[][] = []
  // Nothing kept for the current commit; an older run is on disk.
  wire(on, () => NONE, KEPT, seen)
  await $.command.run({ command: 'journey', args: 'demo' } as any)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'journey-review', surface, component: 'CommandOutput',
      props: { command: 'journey', args: 'demo', text: 'Journey demo', isErrored: false } as any,
    })
    // Opens on the first step that is not pass: step 2, drift, no frame.
    expect(await text(ui, /2. End screen/)).toBeDefined()
    expect(await text(ui, /1 of 2 passed/)).toBeDefined()
    expect(await text(ui, /^Why$/)).toBeDefined()
    expect(await text(ui, /the owner/)).toBeDefined()
    expect(await text(ui, /text Done/)).toBeDefined()
    expect(await text(ui, /the page showed an error/)).toBeDefined()
    expect(await text(ui, /(page_error)/)).toBeDefined()
    expect(await text(ui, /2 steps, checked by a headless browser run.*risks: authentication, security/)).toBeDefined()
    expect(await text(ui, /on abcdef01, .* \(older\)/)).toBeDefined()
    expect(await text(ui, /✗ text Done/)).toBeDefined()
    expect(await text(ui, /after you/)).toBeUndefined()
    if (surface === 'desktop') {
      expect(JSON.stringify(await ui.find({ type: 'Svg' }))).toContain('No screenshot for this step')
    }
    // Pressing a filmstrip item selects that step and redraws the card.
    await ui.press({ key: 'pick-a' })
    expect(await text(ui, /1. Start screen/)).toBeDefined()
    expect(await text(ui, /✓ exit 0/)).toBeDefined()
    if (surface === 'terminal') expect(await ui.find({ type: 'Image' })).toBeDefined()
    else {
      expect(JSON.stringify(await ui.find({ type: 'Svg' }))).toContain('data:image/jpeg;base64')
    }
    if (surface === 'desktop') {
      // No collapsed state: the large frame is a full-width Svg with no fixed size.
      const big: any = await ui.find({ type: 'Svg' })
      expect(JSON.stringify(big)).toContain('data:image/jpeg;base64')
      expect(big.props.width).toBeUndefined()
      expect(await ui.find({ type: 'Button', key: 'expand' })).toBeUndefined()
      expect(await ui.find({ type: 'Button', key: 'close' })).toBeUndefined()
      expect(await text(ui, /1 \/ 2  Start screen/)).toBeDefined()
      await ui.press({ key: 'original' })
      expect(seen.some(a => a[0] === 'explorer.exe')).toBe(true)
      await ui.press({ key: 'next' })
      expect(await text(ui, /2 \/ 2  End screen/)).toBeDefined()
    }
    // One real button per step: "N title", marked with its status.
    expect(await ui.find({ type: 'Button', text: /1 Start screen/ })).toBeDefined()
    expect(await ui.find({ type: 'Button', text: /2 End screen/ })).toBeDefined()
    await ui.press({ key: 'pick-a' })
    expect(await text(ui, /1 \/ 2  Start screen/)).toBeDefined()
    await ui.press({ key: 'approve' })
    await ui.press({ key: 'refresh' })
    await ui.unmount()
  }
  expect(seen.some(a => a.includes('approve') && a.includes('--token-file'))).toBe(true)
  expect(seen.some(a => a.includes('--force'))).toBe(true)
})

test('nothing kept at all shows one line', async ($, on) => {
  wire(on, () => NONE, null, [])
  await $.command.run({ command: 'journey', args: 'demo' } as any)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'journey-review', surface, component: 'CommandOutput',
      props: { command: 'journey', args: 'demo', text: 'Journey demo', isErrored: false } as any,
    })
    expect(await text(ui, /No run yet/)).toBeDefined()
    await ui.unmount()
  }
})

test('show_journey tool result draws the cards', async ($, on) => {
  wire(on, () => KEPT, KEPT, [])
  const ran = await ($.tool as any).call({ tool: 'mcp__journey-review__show_journey', id: 'demo' })
  expect((ran as any).deny).toBeUndefined()
  // The engine validates a tool result: a string (or content array), never an object.
  expect(typeof (ran as any).result).toBe('string')
  expect(JSON.stringify(ran)).toContain('Journey demo shown')
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'journey-review', surface, component: 'ToolResult',
      props: { tool: 'mcp__journey-review__show_journey', tool_use_id: 't1', output: String((ran as any).result), isErrored: false } as any,
      requestId: 't1',
    })
    expect(await text(ui, /End screen/)).toBeDefined()
    expect(await ui.find({ type: 'Button', key: 'approve' })).toBeDefined()
    expect(await ui.find({ type: 'Button', key: 'refresh' })).toBeDefined()
    await ui.unmount()
  }
})

test('show_journey inside a folded ToolGroup still draws the card', async ($, on) => {
  wire(on, () => KEPT, KEPT, [])
  // The engine's own count line sits beneath the plugin.
  on('ui.render', async () => ({ type: 'Text', children: ['Used 2 tools'] }) as any)
  await ($.tool as any).call({ tool: 'mcp__journey-review__show_journey', id: 'demo' })
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({
      plugin: 'journey-review', surface, component: 'ToolGroup',
      props: {
        isActive: false, isExpanded: false,
        calls: [
          { tool: 'Bash', input: {}, isRunning: false, isErrored: false, isInterrupted: false },
          { tool: 'mcp__journey-review__show_journey', input: { id: 'demo' }, isRunning: false, isErrored: false, isInterrupted: false },
        ],
      } as any,
    })
    expect(await text(ui, /Used 2 tools/)).toBeDefined()
    expect(await text(ui, /End screen/)).toBeDefined()
    expect(await ui.find({ type: 'Button', key: 'approve' })).toBeDefined()
    await ui.unmount()
  }
})

const NOW = '2026-10-18T12:00:00Z'
const iso = (minAgo: number) => new Date(Date.parse(NOW) - minAgo * 60000).toISOString()
const task = (issue: number, step: string, lane: string, idleMin: number, extra: any = {}) => ({
  issue, title: `Task ${issue}`, step, lane, pr: step === 'plan' ? null : 100 + issue, key: `issue-${issue}#x`,
  clock: { lastAt: iso(idleMin), since: iso(idleMin + 60) }, lastHeartbeatAt: null, claimedAt: iso(500),
  blockedBy: null, releasedBy: null, ...extra,
})
const BOARD = {
  now: NOW,
  tasks: [
    task(1, 'implement', 'l1', 5), task(2, 'review', 'l2', 45), task(3, 'fix', 'l3', 10),
    task(4, 'implement', 'l4', 3), task(5, 'review', 'l5', 20, { key: 'stalled-5' }), task(6, 'merge', 'l6', 2),
    task(7, 'review', 'l7', 1, { releasedBy: 'x' }),
  ],
  slots: [{ holder: { lane: 'l4' }, waiting: [] }],
  stalls: ['stalled-5'], silent: [],
}
const wireLanes = (on: any, seen: string[][], fail = false) => {
  const clock = mock.clock(on, { now: Date.parse(NOW) })
  on('session.cwd', async (p: any) => ({ value: p.plugin.root }))
  on('ui.open', async (_$: any, e: any) => { opened.push(e); return { value: { isPlaced: true } } })
  on('process.run', async (_$: any, e: any) => {
    seen.push(e.argv)
    if (fail) return { value: { exitCode: 1, stdout: '', stderr: 'down' } }
    return { value: { exitCode: 0, stdout: JSON.stringify(BOARD) + '\n', stderr: '' } }
  })
  return clock
}

test('/lanes opens the panel, groups by column, marks silent tasks red', async ($, on) => {
  const seen: string[][] = []
  wireLanes(on, seen)
  opened.length = 0
  const ran: any = await $.command.run({ command: 'lanes', args: '' } as any)
  expect(JSON.stringify(ran)).toContain('Lanes panel opened')
  expect(opened.some(o => o.id === 'lanes')).toBe(true)
  // the token is passed as a file path only
  expect(seen.some(a => a.some(x => x.endsWith('events.token')))).toBe(true)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ plugin: 'journey-review', surface, component: 'Pane', props: {} as any, requestId: 'lanes' } as any)
    expect(await text(ui, /^Implement 1$/)).toBeDefined()
    expect(await text(ui, /^Build queue 1$/)).toBeDefined()
    expect(await text(ui, /^Silent 1$/)).toBeDefined()
    expect(await text(ui, /^Blocked 1$/)).toBeDefined()
    expect(await text(ui, /^Merge 1$/)).toBeDefined()
    expect(await text(ui, /#1 Task 1/)).toBeDefined()
    expect(await text(ui, /#7 Task 7/)).toBeUndefined() // released: not open
    const stale: any = await ui.find({ type: 'Text', text: /#2 Task 2/ })
    expect(stale.props.color).toBe('red')
    const fresh: any = await ui.find({ type: 'Text', text: /#1 Task 1/ })
    expect(fresh.props.color).toBeUndefined()
    expect(await text(ui, /review · l2 · PR 102 · in step 2h · idle 45m/)).toBeDefined()
    await ui.press({ key: 'toggle-implement' })
    expect(await text(ui, /#1 Task 1/)).toBeUndefined()
    await ui.press({ key: 'toggle-implement' })
    expect(await text(ui, /#1 Task 1/)).toBeDefined()
    await ui.press({ key: 'refresh' })
    await ui.unmount()
  }
})

test('lanes timer refreshes every 30 s and a failed read shows one red line', async ($, on) => {
  const seen: string[][] = []
  const clock = wireLanes(on, seen, true)
  await $.command.run({ command: 'lanes', args: '' } as any)
  const before = seen.length
  await clock.advance(30000)
  expect(seen.length).toBeGreaterThan(before)
  const ui = await $.ui.mount({ plugin: 'journey-review', surface: 'desktop', component: 'Pane', props: {} as any, requestId: 'lanes' } as any)
  expect(await text(ui, /Lanes failed/)).toBeDefined()
  await ui.unmount()
})

test('show_lanes returns a string and opens the panel', async ($, on) => {
  wireLanes(on, [])
  opened.length = 0
  const ran: any = await ($.tool as any).call({ tool: 'mcp__journey-review__show_lanes' })
  expect(typeof ran.result).toBe('string')
  expect(ran.result).toContain('6 open tasks'.replace('6', '6'))
  expect(opened.some(o => o.id === 'lanes')).toBe(true)
})
