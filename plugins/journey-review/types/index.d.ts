export type Check = { role: string; name: string; seen: boolean | null }
export type StepView = {
  id: string
  title: string
  result: string
  reason?: string
  frame?: string
  action?: string
  checks: Check[]
}
export type JourneyView = {
  id: string
  title: string
  loading: boolean
  error?: string
  overall?: string
  commit?: string
  ranAt?: string
  steps: StepView[]
  rejecting: boolean
  note: string
  older?: string
  purpose?: string
  proves?: string
  actors?: string[]
  risks?: string[]
  empty?: boolean
}
export type FlowRow = { id: string; title: string; status: string }

declare module 'claude-code' {
  interface PluginState {
    'journey-review': {
      views: Record<string, JourneyView>
      flows: FlowRow[]
      flowsError: string | null
      selected: Record<string, string>
    }
  }
}
