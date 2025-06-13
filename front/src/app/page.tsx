"use client";
import { useState, useEffect } from 'react';
import {
  Line
} from 'react-chartjs-2';
import {
  Chart as ChartJS,
  LineElement,
  PointElement,
  CategoryScale,
  LinearScale,
  LogarithmicScale,
  Legend,
  Tooltip,
} from 'chart.js';

ChartJS.register(
  LineElement,
  PointElement,
  CategoryScale,
  LinearScale,
  LogarithmicScale,
  Legend,
  Tooltip
);

interface TrainingStatus {
  running: boolean;
  generation: number;
  best_fitness: number;
  done: boolean;
  error?: string | null;
}

interface GenerationSnapshot {
  generation: number;
  fitness: number;
  sharpe_ratio: number;
}

interface TradeLogEntry {
  timestamp: string;
  symbol: string;
  side: string;
  entry_price: number;
  exit_price: number;
  quantity: number;
  pnl: number;
  pnl_pct: number;
  fee: number;
  cash_after_trade: number;
  equity_after_trade: number;
  reason: string;
}

interface EquityPoint {
  timestamp: string;
  equity: number;
}

interface BestStrategyData {
  hash: number;
  trade_log: TradeLogEntry[];
  equity_curve: EquityPoint[];
}

interface KryptoConfig {
  start_date: string;
  api_key?: string;
  api_secret?: string;
  symbols: string[];
  intervals: string[];
  cross_validations: number;
  fee?: number;
  max_n: number;
  max_depth: number;
  generation_limit: number;
  population_size: number;
  mutation_rate: number;
  selection_ratio: number;
  num_individuals_per_parents: number;
  reinsertion_ratio: number;
  technicals: string[];
  margin: number;
  mission: string;
  cache_enabled: boolean;
  backtest_margin_start: number;
  backtest_margin_end: number;
  backtest_margin_step: number;
  walk_forward_train_ratio: number;
  trade_loop_wait_seconds: number;
  trade_qty_percentage: number;
  trade_stop_loss_percentage?: number;
  trade_take_profit_percentage?: number;
}

type ViewMode = 'overview' | 'equity-detail' | 'trades-detail';
type SortField = 'timestamp' | 'symbol' | 'pnl' | 'pnl_pct' | 'entry_price' | 'exit_price';
type SortDirection = 'asc' | 'desc';

export default function Home() {
  const [status, setStatus] = useState<TrainingStatus | null>(null);
  const [history, setHistory] = useState<GenerationSnapshot[]>([]);
  const [best, setBest] = useState<BestStrategyData | null>(null);
  const [config, setConfig] = useState<KryptoConfig | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [isStarting, setIsStarting] = useState(false);
  
  // New state for enhanced views
  const [viewMode, setViewMode] = useState<ViewMode>('overview');
  const [logScale, setLogScale] = useState(false);
  const [equityTimeframe, setEquityTimeframe] = useState<'all' | '7d' | '30d' | '90d'>('all');
  const [tradeFilter, setTradeFilter] = useState({
    symbol: '',
    side: '',
    minPnl: '',
    maxPnl: '',
  });
  const [sortConfig, setSortConfig] = useState<{ field: SortField; direction: SortDirection }>({
    field: 'timestamp',
    direction: 'desc'
  });
  const [currentPage, setCurrentPage] = useState(1);
  const tradesPerPage = 50;

  useEffect(() => {
    async function fetchData() {
      try {
        const statusRes = await fetch('http://localhost:8080/status');
        if (statusRes.ok) {
          setStatus(await statusRes.json());
        }
        const histRes = await fetch('http://localhost:8080/generation');
        if (histRes.ok) {
          setHistory(await histRes.json());
        }
        const bestRes = await fetch('http://localhost:8080/best/data');
        if (bestRes.ok && bestRes.status === 200) {
          const data: BestStrategyData = await bestRes.json();
          setBest(data);
        }
        const configRes = await fetch('http://localhost:8080/config');
        if (configRes.ok) {
          setConfig(await configRes.json());
        }
      } catch (err) {
        console.error(err);
      }
    }
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, []);

  const startOptimization = async () => {
    setIsStarting(true);
    try {
      const response = await fetch('http://localhost:8080/train/start', {
        method: 'POST',
      });
      if (!response.ok) {
        const errorText = await response.text();
        alert(`Failed to start training: ${errorText}`);
      }
    } catch (err) {
      console.error('Failed to start training:', err);
      alert('Failed to start training. Make sure the server is running.');
    } finally {
      setIsStarting(false);
    }
  };

  const updateConfig = async (newConfig: KryptoConfig) => {
    try {
      const response = await fetch('http://localhost:8080/config', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(newConfig),
      });
      if (response.ok) {
        setConfig(newConfig);
        setShowSettings(false);
      } else {
        alert('Failed to update configuration');
      }
    } catch (err) {
      console.error('Failed to update config:', err);
      alert('Failed to update configuration');
    }
  };

  // Filter equity data based on timeframe
  const getFilteredEquityData = () => {
    if (!best?.equity_curve) return [];
    
    if (equityTimeframe === 'all') return best.equity_curve;
    
    const now = new Date();
    const daysBack = {
      '7d': 7,
      '30d': 30,
      '90d': 90
    }[equityTimeframe];
    
    const cutoffDate = new Date(now.getTime() - daysBack * 24 * 60 * 60 * 1000);
    
    return best.equity_curve.filter(point => 
      new Date(point.timestamp) >= cutoffDate
    );
  };

  // Filter and sort trades
  const getFilteredTrades = () => {
    if (!best?.trade_log) return [];
    
    const filtered = best.trade_log.filter(trade => {
      if (tradeFilter.symbol && !trade.symbol.toLowerCase().includes(tradeFilter.symbol.toLowerCase())) return false;
      if (tradeFilter.side && trade.side.toLowerCase() !== tradeFilter.side.toLowerCase()) return false;
      if (tradeFilter.minPnl && trade.pnl < parseFloat(tradeFilter.minPnl)) return false;
      if (tradeFilter.maxPnl && trade.pnl > parseFloat(tradeFilter.maxPnl)) return false;
      return true;
    });

    // Sort trades
    filtered.sort((a, b) => {
      let aVal: string | number = a[sortConfig.field];
      let bVal: string | number = b[sortConfig.field];
      
      if (sortConfig.field === 'timestamp') {
        aVal = new Date(aVal).getTime();
        bVal = new Date(bVal).getTime();
      }
      
      if (typeof aVal === 'string' && typeof bVal === 'string') {
        aVal = aVal.toLowerCase();
        bVal = bVal.toLowerCase();
      }
      
      if (sortConfig.direction === 'asc') {
        return aVal < bVal ? -1 : aVal > bVal ? 1 : 0;
      } else {
        return aVal > bVal ? -1 : aVal < bVal ? 1 : 0;
      }
    });

    return filtered;
  };

  const handleSort = (field: SortField) => {
    setSortConfig(prev => ({
      field,
      direction: prev.field === field && prev.direction === 'desc' ? 'asc' : 'desc'
    }));
    setCurrentPage(1);
  };

  const fitnessData = {
    labels: history.map((h) => h.generation.toString()),
    datasets: [
      {
        label: 'Fitness',
        data: history.map((h) => h.fitness),
        borderColor: 'rgb(99, 102, 241)',
        backgroundColor: 'rgba(99, 102, 241, 0.1)',
        tension: 0.1,
      },
      {
        label: 'Sharpe Ratio',
        data: history.map((h) => h.sharpe_ratio),
        borderColor: 'rgb(16, 185, 129)',
        backgroundColor: 'rgba(16, 185, 129, 0.1)',
        yAxisID: 'y1',
        tension: 0.1,
      },
    ],
  };

  const fitnessOptions = {
    responsive: true,
    interaction: {
      mode: 'index' as const,
      intersect: false,
    },
    scales: {
      y: { 
        title: { display: true, text: 'Fitness' },
        grid: { color: 'rgba(99, 102, 241, 0.1)' }
      },
      y1: {
        position: 'right' as const,
        title: { display: true, text: 'Sharpe Ratio' },
        grid: { drawOnChartArea: false },
      },
      x: {
        grid: { color: 'rgba(156, 163, 175, 0.1)' }
      }
    },
    plugins: {
      legend: {
        position: 'top' as const,
      },
    },
  };

  const equityData = () => {
    const filteredData = getFilteredEquityData();
    return {
      labels: filteredData.map((e) => new Date(e.timestamp).toLocaleString()),
      datasets: [
        {
          label: 'Portfolio Equity',
          data: filteredData.map((e) => e.equity),
          borderColor: 'rgb(239, 68, 68)',
          backgroundColor: 'rgba(239, 68, 68, 0.1)',
          tension: 0.1,
          fill: true,
          pointRadius: viewMode === 'equity-detail' ? 2 : 0,
          pointHoverRadius: 4,
        },
      ],
    };
  };

  const equityOptions = { 
    responsive: true,
    plugins: {
      legend: {
        position: 'top' as const,
      },
      tooltip: {
        callbacks: {
          label: (context: { parsed: { y: number }; label: string }) => {
            const value = context.parsed.y;
            const timestamp = context.label;
            return [
              `Equity: $${value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`,
              `Time: ${timestamp}`,
            ];
          }
        }
      }
    },
    scales: {
      x: {
        grid: { color: 'rgba(156, 163, 175, 0.1)' },
        display: viewMode === 'equity-detail',
      },
      y: {
        type: logScale ? 'logarithmic' as const : 'linear' as const,
        grid: { color: 'rgba(239, 68, 68, 0.1)' },
        title: {
          display: true,
          text: `Portfolio Value (${logScale ? 'Log Scale' : 'Linear'})`
        }
      }
    },
    onClick: (event: any, elements: any[]) => {
      if (elements.length > 0 && viewMode === 'overview') {
        setViewMode('equity-detail');
      }
    }
  };

  // Calculate trade statistics
  const tradeStats = () => {
    if (!best?.trade_log?.length) return null;
    
    const trades = best.trade_log;
    const totalTrades = trades.length;
    const winningTrades = trades.filter(t => t.pnl > 0).length;
    const losingTrades = trades.filter(t => t.pnl < 0).length;
    const totalPnl = trades.reduce((sum, t) => sum + t.pnl, 0);
    const avgPnl = totalPnl / totalTrades;
    const maxWin = Math.max(...trades.map(t => t.pnl));
    const maxLoss = Math.min(...trades.map(t => t.pnl));
    const winRate = (winningTrades / totalTrades) * 100;
    
    return {
      totalTrades,
      winningTrades,
      losingTrades,
      totalPnl,
      avgPnl,
      maxWin,
      maxLoss,
      winRate
    };
  };

  const stats = tradeStats();
  const filteredTrades = getFilteredTrades();
  const totalPages = Math.ceil(filteredTrades.length / tradesPerPage);
  const startIndex = (currentPage - 1) * tradesPerPage;
  const paginatedTrades = filteredTrades.slice(startIndex, startIndex + tradesPerPage);

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 dark:from-slate-900 dark:to-slate-800">
      <div className="container mx-auto px-6 py-8">
        {/* Header */}
        <div className="flex items-center justify-between mb-8">
          <div>
            <h1 className="text-4xl font-bold text-slate-900 dark:text-white mb-2">
              🚀 Krypto Trading Optimizer
            </h1>
            <p className="text-slate-600 dark:text-slate-300">
              AI-powered cryptocurrency trading strategy optimization using genetic algorithms
            </p>
          </div>
          <div className="flex gap-3">
            <button
              onClick={() => setViewMode('overview')}
              className={`px-4 py-2 rounded-lg transition-colors duration-200 ${
                viewMode === 'overview' 
                  ? 'bg-blue-600 text-white' 
                  : 'bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 hover:bg-slate-300 dark:hover:bg-slate-600'
              }`}
            >
              📊 Overview
            </button>
            <button
              onClick={() => setShowSettings(true)}
              className="px-4 py-2 bg-slate-600 hover:bg-slate-700 text-white rounded-lg transition-colors duration-200 flex items-center gap-2"
            >
              ⚙️ Settings
            </button>
            <button
              onClick={startOptimization}
              disabled={status?.running || isStarting}
              className={`px-6 py-2 rounded-lg font-medium transition-all duration-200 flex items-center gap-2 ${
                status?.running || isStarting
                  ? 'bg-gray-400 cursor-not-allowed text-gray-600'
                  : 'bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white shadow-lg hover:shadow-xl'
              }`}
            >
              {status?.running ? '🔄 Running...' : isStarting ? '🚀 Starting...' : '▶️ Start Optimization'}
            </button>
          </div>
        </div>

        {/* Status Cards */}
        <div className="grid grid-cols-1 md:grid-cols-4 gap-6 mb-8">
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center gap-3">
              <div className={`w-3 h-3 rounded-full ${status?.running ? 'bg-green-500 animate-pulse' : status?.done ? 'bg-blue-500' : 'bg-gray-400'}`}></div>
              <div>
                <p className="text-sm text-slate-600 dark:text-slate-400">Status</p>
                <p className="font-semibold text-slate-900 dark:text-white">
                  {status?.running ? 'Running' : status?.done ? 'Completed' : 'Idle'}
                </p>
              </div>
            </div>
          </div>

          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center gap-3">
              <div className="text-2xl">🧬</div>
              <div>
                <p className="text-sm text-slate-600 dark:text-slate-400">Generation</p>
                <p className="text-2xl font-bold text-slate-900 dark:text-white">
                  {status?.generation || 0}
                </p>
              </div>
            </div>
          </div>

          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center gap-3">
              <div className="text-2xl">💪</div>
              <div>
                <p className="text-sm text-slate-600 dark:text-slate-400">Best Fitness</p>
                <p className="text-2xl font-bold text-blue-600 dark:text-blue-400">
                  {status?.best_fitness?.toLocaleString() || '0'}
                </p>
              </div>
            </div>
          </div>

          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center gap-3">
              <div className="text-2xl">📊</div>
              <div>
                <p className="text-sm text-slate-600 dark:text-slate-400">Strategies Tested</p>
                <p className="text-2xl font-bold text-purple-600 dark:text-purple-400">
                  {history.length > 0 ? (history[history.length - 1].generation * (config?.population_size || 50)).toLocaleString() : '0'}
                </p>
              </div>
            </div>
          </div>
        </div>

        {status?.error && (
          <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-xl p-4 mb-8">
            <div className="flex items-center gap-2">
              <span className="text-red-500">⚠️</span>
              <p className="text-red-700 dark:text-red-300 font-medium">Error: {status.error}</p>
            </div>
          </div>
        )}

        {viewMode === 'overview' && (
          <>
            {/* Charts */}
            <div className="grid grid-cols-1 xl:grid-cols-2 gap-8 mb-8">
              <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
                <h3 className="text-xl font-semibold text-slate-900 dark:text-white mb-4 flex items-center gap-2">
                  📈 Optimization Progress
                </h3>
                {history.length > 0 ? (
                  <Line data={fitnessData} options={fitnessOptions} />
                ) : (
                  <div className="h-64 flex items-center justify-center text-slate-500 dark:text-slate-400">
                    <div className="text-center">
                      <div className="text-4xl mb-2">📊</div>
                      <p>No optimization data yet</p>
                      <p className="text-sm">Start optimization to see progress</p>
                    </div>
                  </div>
                )}
              </div>

              <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
                <div className="flex items-center justify-between mb-4">
                  <h3 className="text-xl font-semibold text-slate-900 dark:text-white flex items-center gap-2">
                    💰 Best Strategy Equity Curve
                  </h3>
                  {best?.equity_curve?.length && (
                    <button
                      onClick={() => setViewMode('equity-detail')}
                      className="text-sm bg-blue-600 hover:bg-blue-700 text-white px-3 py-1 rounded-lg transition-colors"
                    >
                      📊 Detailed View
                    </button>
                  )}
                </div>
                {best?.equity_curve?.length ? (
                  <Line data={equityData()} options={equityOptions} />
                ) : (
                  <div className="h-64 flex items-center justify-center text-slate-500 dark:text-slate-400">
                    <div className="text-center">
                      <div className="text-4xl mb-2">💹</div>
                      <p>No equity data available</p>
                      <p className="text-sm">Run optimization to generate strategies</p>
                    </div>
                  </div>
                )}
              </div>
            </div>

            {/* Trade Statistics Overview */}
            {stats && (
              <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6 mb-8">
                <div className="flex items-center justify-between mb-4">
                  <h3 className="text-xl font-semibold text-slate-900 dark:text-white flex items-center gap-2">
                    📈 Trading Performance Summary
                  </h3>
                  <button
                    onClick={() => setViewMode('trades-detail')}
                    className="text-sm bg-green-600 hover:bg-green-700 text-white px-3 py-1 rounded-lg transition-colors"
                  >
                    📋 View All Trades
                  </button>
                </div>
                <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-7 gap-4">
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Total Trades</p>
                    <p className="text-2xl font-bold text-slate-900 dark:text-white">{stats.totalTrades}</p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Win Rate</p>
                    <p className="text-2xl font-bold text-green-600 dark:text-green-400">{stats.winRate.toFixed(1)}%</p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Total P&L</p>
                    <p className={`text-2xl font-bold ${stats.totalPnl >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                      ${stats.totalPnl.toFixed(2)}
                    </p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Avg P&L</p>
                    <p className={`text-2xl font-bold ${stats.avgPnl >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                      ${stats.avgPnl.toFixed(2)}
                    </p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Best Trade</p>
                    <p className="text-2xl font-bold text-green-600 dark:text-green-400">${stats.maxWin.toFixed(2)}</p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Worst Trade</p>
                    <p className="text-2xl font-bold text-red-600 dark:text-red-400">${stats.maxLoss.toFixed(2)}</p>
                  </div>
                  <div className="text-center">
                    <p className="text-sm text-slate-600 dark:text-slate-400">Winning Trades</p>
                    <p className="text-2xl font-bold text-green-600 dark:text-green-400">{stats.winningTrades}</p>
                  </div>
                </div>
              </div>
            )}

            {/* Recent Trades Preview */}
            {best && best.trade_log && best.trade_log.length > 0 && (
              <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
                <h3 className="text-xl font-semibold text-slate-900 dark:text-white mb-4 flex items-center gap-2">
                  📋 Recent Trades (Last 10)
                </h3>
                <div className="overflow-x-auto">
                  <table className="min-w-full">
                    <thead>
                      <tr className="border-b border-slate-200 dark:border-slate-700">
                        <th className="text-left py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Time</th>
                        <th className="text-left py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Symbol</th>
                        <th className="text-left py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Side</th>
                        <th className="text-right py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Entry</th>
                        <th className="text-right py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Exit</th>
                        <th className="text-right py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">P&L</th>
                        <th className="text-right py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">P&L %</th>
                      </tr>
                    </thead>
                    <tbody>
                      {best.trade_log.slice(-10).map((trade, idx) => (
                        <tr key={idx} className="border-b border-slate-100 dark:border-slate-700 hover:bg-slate-50 dark:hover:bg-slate-700 transition-colors">
                          <td className="py-3 px-4 text-slate-900 dark:text-white text-sm">
                            {new Date(trade.timestamp).toLocaleString()}
                          </td>
                          <td className="py-3 px-4 text-slate-900 dark:text-white font-medium">{trade.symbol}</td>
                          <td className="py-3 px-4">
                            <span className={`px-2 py-1 rounded-full text-xs font-medium ${
                              trade.side === 'Buy' ? 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200' : 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200'
                            }`}>
                              {trade.side}
                            </span>
                          </td>
                          <td className="py-3 px-4 text-right text-slate-900 dark:text-white">${trade.entry_price.toFixed(2)}</td>
                          <td className="py-3 px-4 text-right text-slate-900 dark:text-white">${trade.exit_price.toFixed(2)}</td>
                          <td className={`py-3 px-4 text-right font-medium ${trade.pnl >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                            ${trade.pnl.toFixed(2)}
                          </td>
                          <td className={`py-3 px-4 text-right font-medium ${trade.pnl_pct >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                            {trade.pnl_pct.toFixed(2)}%
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </>
        )}

        {viewMode === 'equity-detail' && best?.equity_curve?.length && (
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-xl font-semibold text-slate-900 dark:text-white flex items-center gap-2">
                📊 Detailed Equity Curve Analysis
              </h3>
              <div className="flex gap-2">
                <button
                  onClick={() => setViewMode('overview')}
                  className="px-3 py-1 text-sm bg-slate-600 hover:bg-slate-700 text-white rounded-lg transition-colors"
                >
                  ← Back
                </button>
              </div>
            </div>
            
            {/* Equity Controls */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-2">
                  Time Range
                </label>
                <div className="flex gap-2">
                  {(['all', '7d', '30d', '90d'] as const).map(period => (
                    <button
                      key={period}
                      onClick={() => setEquityTimeframe(period)}
                      className={`px-3 py-2 text-sm rounded-lg transition-colors ${
                        equityTimeframe === period
                          ? 'bg-blue-600 text-white'
                          : 'bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 hover:bg-slate-300 dark:hover:bg-slate-600'
                      }`}
                    >
                      {period === 'all' ? 'All Time' : period.toUpperCase()}
                    </button>
                  ))}
                </div>
              </div>
              
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-2">
                  Scale Type
                </label>
                <div className="flex gap-2">
                  <button
                    onClick={() => setLogScale(false)}
                    className={`px-3 py-2 text-sm rounded-lg transition-colors ${
                      !logScale
                        ? 'bg-blue-600 text-white'
                        : 'bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 hover:bg-slate-300 dark:hover:bg-slate-600'
                    }`}
                  >
                    Linear
                  </button>
                  <button
                    onClick={() => setLogScale(true)}
                    className={`px-3 py-2 text-sm rounded-lg transition-colors ${
                      logScale
                        ? 'bg-blue-600 text-white'
                        : 'bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 hover:bg-slate-300 dark:hover:bg-slate-600'
                    }`}
                  >
                    Logarithmic
                  </button>
                </div>
              </div>
            </div>

            {/* Detailed Chart */}
            <div className="h-96">
              <Line data={equityData()} options={equityOptions} />
            </div>
            
            {/* Equity Statistics */}
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mt-6 p-4 bg-slate-50 dark:bg-slate-700 rounded-lg">
              <div className="text-center">
                <p className="text-sm text-slate-600 dark:text-slate-400">Start Value</p>
                <p className="text-lg font-bold text-slate-900 dark:text-white">
                  ${getFilteredEquityData()[0]?.equity.toFixed(2) || '0.00'}
                </p>
              </div>
              <div className="text-center">
                <p className="text-sm text-slate-600 dark:text-slate-400">End Value</p>
                <p className="text-lg font-bold text-slate-900 dark:text-white">
                  ${getFilteredEquityData()[getFilteredEquityData().length - 1]?.equity.toFixed(2) || '0.00'}
                </p>
              </div>
              <div className="text-center">
                <p className="text-sm text-slate-600 dark:text-slate-400">Total Return</p>
                <p className="text-lg font-bold text-green-600 dark:text-green-400">
                  {getFilteredEquityData().length > 1 ? 
                    (((getFilteredEquityData()[getFilteredEquityData().length - 1].equity / getFilteredEquityData()[0].equity) - 1) * 100).toFixed(2) + '%'
                    : '0.00%'
                  }
                </p>
              </div>
              <div className="text-center">
                <p className="text-sm text-slate-600 dark:text-slate-400">Data Points</p>
                <p className="text-lg font-bold text-slate-900 dark:text-white">
                  {getFilteredEquityData().length.toLocaleString()}
                </p>
              </div>
            </div>
          </div>
        )}

        {viewMode === 'trades-detail' && (
          <div className="bg-white dark:bg-slate-800 rounded-xl shadow-sm border border-slate-200 dark:border-slate-700 p-6">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-xl font-semibold text-slate-900 dark:text-white flex items-center gap-2">
                📋 All Trades Analysis
              </h3>
              <div className="flex gap-2">
                <button
                  onClick={() => setViewMode('overview')}
                  className="px-3 py-1 text-sm bg-slate-600 hover:bg-slate-700 text-white rounded-lg transition-colors"
                >
                  ← Back
                </button>
              </div>
            </div>

            {/* Trade Filters */}
            <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6 p-4 bg-slate-50 dark:bg-slate-700 rounded-lg">
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Symbol
                </label>
                <input
                  type="text"
                  value={tradeFilter.symbol}
                  onChange={(e) => setTradeFilter(prev => ({ ...prev, symbol: e.target.value }))}
                  placeholder="e.g., BTC"
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 text-slate-900 dark:text-white text-sm"
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Side
                </label>
                <select
                  value={tradeFilter.side}
                  onChange={(e) => setTradeFilter(prev => ({ ...prev, side: e.target.value }))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 text-slate-900 dark:text-white text-sm"
                >
                  <option value="">All</option>
                  <option value="buy">Buy</option>
                  <option value="sell">Sell</option>
                </select>
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Min P&L
                </label>
                <input
                  type="number"
                  value={tradeFilter.minPnl}
                  onChange={(e) => setTradeFilter(prev => ({ ...prev, minPnl: e.target.value }))}
                  placeholder="Min profit"
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 text-slate-900 dark:text-white text-sm"
                />
              </div>
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Max P&L
                </label>
                <input
                  type="number"
                  value={tradeFilter.maxPnl}
                  onChange={(e) => setTradeFilter(prev => ({ ...prev, maxPnl: e.target.value }))}
                  placeholder="Max profit"
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-800 text-slate-900 dark:text-white text-sm"
                />
              </div>
            </div>

            {/* Trade Table */}
            <div className="overflow-x-auto">
              <table className="min-w-full">
                <thead>
                  <tr className="border-b border-slate-200 dark:border-slate-700">
                    {[
                      { field: 'timestamp', label: 'Time' },
                      { field: 'symbol', label: 'Symbol' },
                      { field: 'side', label: 'Side' },
                      { field: 'entry_price', label: 'Entry' },
                      { field: 'exit_price', label: 'Exit' },
                      { field: 'pnl', label: 'P&L' },
                      { field: 'pnl_pct', label: 'P&L %' },
                    ].map(({ field, label }) => (
                      <th
                        key={field}
                        className="text-left py-3 px-4 text-slate-600 dark:text-slate-400 font-medium cursor-pointer hover:bg-slate-100 dark:hover:bg-slate-700"
                        onClick={() => handleSort(field as SortField)}
                      >
                        <div className="flex items-center gap-1">
                          {label}
                          {sortConfig.field === field && (
                            <span className="text-xs">
                              {sortConfig.direction === 'asc' ? '↑' : '↓'}
                            </span>
                          )}
                        </div>
                      </th>
                    ))}
                    <th className="text-left py-3 px-4 text-slate-600 dark:text-slate-400 font-medium">Reason</th>
                  </tr>
                </thead>
                <tbody>
                  {paginatedTrades.map((trade, idx) => (
                    <tr key={idx} className="border-b border-slate-100 dark:border-slate-700 hover:bg-slate-50 dark:hover:bg-slate-700 transition-colors">
                      <td className="py-3 px-4 text-slate-900 dark:text-white text-sm">
                        {new Date(trade.timestamp).toLocaleString()}
                      </td>
                      <td className="py-3 px-4 text-slate-900 dark:text-white font-medium">{trade.symbol}</td>
                      <td className="py-3 px-4">
                        <span className={`px-2 py-1 rounded-full text-xs font-medium ${
                          trade.side === 'Buy' ? 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200' : 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200'
                        }`}>
                          {trade.side}
                        </span>
                      </td>
                      <td className="py-3 px-4 text-right text-slate-900 dark:text-white">${trade.entry_price.toFixed(4)}</td>
                      <td className="py-3 px-4 text-right text-slate-900 dark:text-white">${trade.exit_price.toFixed(4)}</td>
                      <td className={`py-3 px-4 text-right font-medium ${trade.pnl >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                        ${trade.pnl.toFixed(4)}
                      </td>
                      <td className={`py-3 px-4 text-right font-medium ${trade.pnl_pct >= 0 ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}`}>
                        {trade.pnl_pct.toFixed(2)}%
                      </td>
                      <td className="py-3 px-4 text-slate-600 dark:text-slate-400 text-sm">{trade.reason}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {/* Pagination */}
            {totalPages > 1 && (
              <div className="flex items-center justify-between mt-6">
                <p className="text-sm text-slate-600 dark:text-slate-400">
                  Showing {startIndex + 1} to {Math.min(startIndex + tradesPerPage, filteredTrades.length)} of {filteredTrades.length} trades
                </p>
                <div className="flex gap-2">
                  <button
                    onClick={() => setCurrentPage(Math.max(1, currentPage - 1))}
                    disabled={currentPage === 1}
                    className="px-3 py-2 text-sm bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 rounded-lg disabled:opacity-50 disabled:cursor-not-allowed hover:bg-slate-300 dark:hover:bg-slate-600 transition-colors"
                  >
                    Previous
                  </button>
                  <span className="px-3 py-2 text-sm text-slate-600 dark:text-slate-400">
                    Page {currentPage} of {totalPages}
                  </span>
                  <button
                    onClick={() => setCurrentPage(Math.min(totalPages, currentPage + 1))}
                    disabled={currentPage === totalPages}
                    className="px-3 py-2 text-sm bg-slate-200 dark:bg-slate-700 text-slate-700 dark:text-slate-300 rounded-lg disabled:opacity-50 disabled:cursor-not-allowed hover:bg-slate-300 dark:hover:bg-slate-600 transition-colors"
                  >
                    Next
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Settings Modal */}
      {showSettings && config && (
        <SettingsModal
          config={config}
          onSave={updateConfig}
          onClose={() => setShowSettings(false)}
        />
      )}
    </div>
  );
}

interface SettingsModalProps {
  config: KryptoConfig;
  onSave: (config: KryptoConfig) => void;
  onClose: () => void;
}

function SettingsModal({ config, onSave, onClose }: SettingsModalProps) {
  const [formConfig, setFormConfig] = useState<KryptoConfig>({ ...config });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSave(formConfig);
  };

  const updateField = (field: keyof KryptoConfig, value: string | number | boolean | null) => {
    setFormConfig(prev => ({ ...prev, [field]: value }));
  };

  const updateArrayField = (field: keyof KryptoConfig, value: string) => {
    const array = value.split(',').map(s => s.trim()).filter(s => s);
    setFormConfig(prev => ({ ...prev, [field]: array }));
  };

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
      <div className="bg-white dark:bg-slate-800 rounded-xl shadow-xl max-w-4xl w-full max-h-[90vh] overflow-y-auto">
        <div className="p-6 border-b border-slate-200 dark:border-slate-700">
          <h2 className="text-2xl font-bold text-slate-900 dark:text-white">⚙️ Optimization Settings</h2>
        </div>
        
        <form onSubmit={handleSubmit} className="p-6 space-y-6">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {/* Genetic Algorithm Parameters */}
            <div className="space-y-4">
              <h3 className="text-lg font-semibold text-slate-900 dark:text-white">🧬 Genetic Algorithm</h3>
              
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Generation Limit
                </label>
                <input
                  type="number"
                  value={formConfig.generation_limit}
                  onChange={(e) => updateField('generation_limit', parseInt(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Population Size
                </label>
                <input
                  type="number"
                  value={formConfig.population_size}
                  onChange={(e) => updateField('population_size', parseInt(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Mutation Rate
                </label>
                <input
                  type="number"
                  step="0.01"
                  value={formConfig.mutation_rate}
                  onChange={(e) => updateField('mutation_rate', parseFloat(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="0"
                  max="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Max PLS Components
                </label>
                <input
                  type="number"
                  value={formConfig.max_n}
                  onChange={(e) => updateField('max_n', parseInt(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Max Depth (Lookback)
                </label>
                <input
                  type="number"
                  value={formConfig.max_depth}
                  onChange={(e) => updateField('max_depth', parseInt(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="1"
                />
              </div>
            </div>

            {/* Data & Trading Parameters */}
            <div className="space-y-4">
              <h3 className="text-lg font-semibold text-slate-900 dark:text-white">📊 Data & Trading</h3>
              
              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Symbols (comma-separated)
                </label>
                <textarea
                  value={formConfig.symbols.join(', ')}
                  onChange={(e) => updateArrayField('symbols', e.target.value)}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  rows={3}
                  placeholder="BTCUSDT, ETHUSDT, ADAUSDT"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Intervals (comma-separated)
                </label>
                <input
                  type="text"
                  value={formConfig.intervals.join(', ')}
                  onChange={(e) => updateArrayField('intervals', e.target.value)}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  placeholder="1h, 4h, 1d"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Cross Validations
                </label>
                <input
                  type="number"
                  value={formConfig.cross_validations}
                  onChange={(e) => updateField('cross_validations', parseInt(e.target.value))}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Trading Fee
                </label>
                <input
                  type="number"
                  step="0.0001"
                  value={formConfig.fee || 0}
                  onChange={(e) => updateField('fee', parseFloat(e.target.value) || null)}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                  min="0"
                  max="1"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 dark:text-slate-300 mb-1">
                  Start Date
                </label>
                <input
                  type="date"
                  value={formConfig.start_date}
                  onChange={(e) => updateField('start_date', e.target.value)}
                  className="w-full px-3 py-2 border border-slate-300 dark:border-slate-600 rounded-lg bg-white dark:bg-slate-700 text-slate-900 dark:text-white"
                />
              </div>
            </div>
          </div>

          <div className="flex justify-end gap-3 pt-6 border-t border-slate-200 dark:border-slate-700">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-slate-600 dark:text-slate-400 hover:text-slate-800 dark:hover:text-slate-200 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              className="px-6 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition-colors duration-200"
            >
              Save Settings
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
