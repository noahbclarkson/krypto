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
  Legend,
  Tooltip,
} from 'chart.js';

ChartJS.register(
  LineElement,
  PointElement,
  CategoryScale,
  LinearScale,
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

export default function Home() {
  const [status, setStatus] = useState<TrainingStatus | null>(null);
  const [history, setHistory] = useState<GenerationSnapshot[]>([]);
  const [best, setBest] = useState<BestStrategyData | null>(null);

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
      } catch (err) {
        console.error(err);
      }
    }
    fetchData();
    const interval = setInterval(fetchData, 5000);
    return () => clearInterval(interval);
  }, []);

  const fitnessData = {
    labels: history.map((h) => h.generation.toString()),
    datasets: [
      {
        label: 'Fitness',
        data: history.map((h) => h.fitness),
        borderColor: 'rgb(99, 102, 241)',
        backgroundColor: 'rgba(99, 102, 241, 0.5)',
      },
      {
        label: 'Sharpe',
        data: history.map((h) => h.sharpe_ratio),
        borderColor: 'rgb(16, 185, 129)',
        backgroundColor: 'rgba(16, 185, 129, 0.5)',
        yAxisID: 'y1',
      },
    ],
  };

  const fitnessOptions = {
    responsive: true,
    scales: {
      y: { title: { display: true, text: 'Fitness' } },
      y1: {
        position: 'right' as const,
        title: { display: true, text: 'Sharpe' },
        grid: { drawOnChartArea: false },
      },
    },
  };

  const equityData = {
    labels: best?.equity_curve.map((e) => new Date(e.timestamp).toLocaleTimeString()) || [],
    datasets: [
      {
        label: 'Equity',
        data: best?.equity_curve.map((e) => e.equity) || [],
        borderColor: 'rgb(239, 68, 68)',
        backgroundColor: 'rgba(239, 68, 68, 0.5)',
      },
    ],
  };

  const equityOptions = { responsive: true };

  return (
    <main className="p-6 space-y-6">
      <h1 className="text-2xl font-bold">Krypto Training Dashboard</h1>
      {status && (
        <div className="space-y-2">
          <p>Running: {status.running ? 'Yes' : 'No'}</p>
          <p>Generation: {status.generation}</p>
          <p>Best Fitness: {status.best_fitness}</p>
          {status.error && <p className="text-red-500">Error: {status.error}</p>}
        </div>
      )}
      <div className="space-y-4">
        <h2 className="font-semibold">Generation History</h2>
        <Line data={fitnessData} options={fitnessOptions} />
      </div>

      {best && (
        <div className="space-y-4">
          <h2 className="font-semibold">Best Strategy Equity</h2>
          <Line data={equityData} options={equityOptions} />

          <h3 className="font-medium">Recent Trades</h3>
          <div className="overflow-x-auto">
            <table className="min-w-full text-sm border border-gray-300">
              <thead>
                <tr className="bg-gray-100">
                  <th className="p-2">Time</th>
                  <th className="p-2">Symbol</th>
                  <th className="p-2">Side</th>
                  <th className="p-2">Entry</th>
                  <th className="p-2">Exit</th>
                  <th className="p-2">PnL</th>
                </tr>
              </thead>
              <tbody>
                {best.trade_log.slice(-10).map((t, idx) => (
                  <tr key={idx} className="odd:bg-white even:bg-gray-50">
                    <td className="p-2 whitespace-nowrap">
                      {new Date(t.timestamp).toLocaleString()}
                    </td>
                    <td className="p-2">{t.symbol}</td>
                    <td className="p-2">{t.side}</td>
                    <td className="p-2">{t.entry_price.toFixed(2)}</td>
                    <td className="p-2">{t.exit_price.toFixed(2)}</td>
                    <td className="p-2">{t.pnl.toFixed(2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </main>
  );
}
