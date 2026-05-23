import { Component } from 'react'
import type { ErrorInfo, ReactNode } from 'react'

interface Props {
  children: ReactNode
}

interface State {
  hasError: boolean
  error:    Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary]', error, info.componentStack)
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center py-24 gap-6 text-center">
          <div className="text-5xl">⚠️</div>
          <div>
            <p className="text-red-400 font-semibold text-lg mb-2">
              Une erreur inattendue s'est produite
            </p>
            <p className="text-slate-500 text-sm font-mono max-w-lg">
              {this.state.error?.message ?? 'Erreur inconnue'}
            </p>
          </div>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            className="px-4 py-2 bg-blue-600/20 text-blue-400 border border-blue-500/30
                       rounded-lg text-sm hover:bg-blue-600/30 transition-colors"
          >
            Réessayer
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
