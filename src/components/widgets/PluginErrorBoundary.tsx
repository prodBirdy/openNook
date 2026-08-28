import { Component, ErrorInfo, ReactNode } from 'react';
import { IconAlertTriangle } from '@tabler/icons-react';

interface Props {
    children: ReactNode;
    pluginId: string;
    pluginName: string;
}

interface State {
    hasError: boolean;
    error: Error | null;
    errorInfo: ErrorInfo | null;
}

/**
 * Error boundary to catch and handle plugin crashes gracefully
 * Prevents one broken plugin from breaking the entire app
 */
export class PluginErrorBoundary extends Component<Props, State> {
    constructor(props: Props) {
        super(props);
        this.state = {
            hasError: false,
            error: null,
            errorInfo: null
        };
    }

    static getDerivedStateFromError(_error: Error): Partial<State> {
        return { hasError: true };
    }

    componentDidCatch(error: Error, errorInfo: ErrorInfo) {
        console.error(`Plugin "${this.props.pluginName}" (${this.props.pluginId}) crashed:`, error, errorInfo);
        this.setState({
            error,
            errorInfo
        });
    }

    handleReset = () => {
        this.setState({
            hasError: false,
            error: null,
            errorInfo: null
        });
    };

    render() {
        if (this.state.hasError) {
            return (
                <div className="flex flex-col items-center justify-center gap-4 p-6 bg-red-500/10 border border-red-500/20 rounded-lg">
                    <div className="flex items-center gap-2 text-red-400">
                        <IconAlertTriangle size={24} />
                        <h3 className="text-lg font-semibold">Plugin Error</h3>
                    </div>
                    <div className="text-center text-white/70">
                        <p className="font-medium">{this.props.pluginName}</p>
                        <p className="text-sm mt-1">
                            {this.state.error?.message || 'An unexpected error occurred'}
                        </p>
                    </div>
                    <button
                        onClick={this.handleReset}
                        className="px-4 py-2 bg-red-500/20 hover:bg-red-500/30 border border-red-500/40 rounded-lg text-white text-sm transition-colors"
                    >
                        Try Again
                    </button>
                    {process.env.NODE_ENV === 'development' && this.state.errorInfo && (
                        <details className="mt-2 text-xs text-white/50 max-w-full overflow-auto">
                            <summary className="cursor-pointer hover:text-white/70">
                                Stack Trace
                            </summary>
                            <pre className="mt-2 p-2 bg-black/20 rounded text-left">
                                {this.state.errorInfo.componentStack}
                            </pre>
                        </details>
                    )}
                </div>
            );
        }

        return this.props.children;
    }
}
