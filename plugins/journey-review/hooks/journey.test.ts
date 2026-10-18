import { test, expect } from 'claude-code/testing'

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
