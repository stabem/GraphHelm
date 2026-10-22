import { Component, type ReactNode } from "react";

interface Props {
  /** The view's name, said in the error. */
  name: string;
  children: ReactNode;
}

/** #1899: a view that throws while rendering shows the error in its own region; the sidebar and
 * the top bar stay, so the owner can pick another run or view. Without it React unmounts the whole
 * Studio and leaves an empty page. Keyed by run and view in App, so moving away clears it. */
export class ViewBoundary extends Component<Props, { error: string | null }> {
  state = { error: null as string | null };

  static getDerivedStateFromError(error: unknown) {
    return { error: error instanceof Error ? error.message : String(error) };
  }

  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <section aria-label={this.props.name} className="view">
        <p role="alert" className="error">The {this.props.name} view could not show this run: {this.state.error}</p>
      </section>
    );
  }
}
