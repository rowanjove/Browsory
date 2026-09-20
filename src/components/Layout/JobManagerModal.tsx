import React, { useEffect, useState, useCallback } from 'react';
import {
  X,
  RefreshCw,
  Clock,
  CheckCircle2,
  AlertCircle,
  Ban,
  Activity,
  Layers,
  Cpu,
  FileDown,
  Cloud,
} from 'lucide-react';
import { tauriApi } from '../../services/tauri';
import { BackgroundJob } from '../../types';

interface JobManagerModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const JobManagerModal: React.FC<JobManagerModalProps> = ({ isOpen, onClose }) => {
  const [jobs, setJobs] = useState<BackgroundJob[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState<'all' | 'running' | 'completed' | 'cancelled'>('all');
  const [cancellingId, setCancellingId] = useState<string | null>(null);

  const fetchJobs = useCallback(async () => {
    try {
      const data = await tauriApi.listBackgroundJobs(50);
      setJobs(data);
    } catch (err) {
      console.error('Failed to list background jobs:', err);
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    setLoading(true);
    fetchJobs().finally(() => setLoading(false));

    // 弹窗开启时，每 2.5 秒轮询更新一次状态
    const interval = setInterval(() => {
      fetchJobs();
    }, 2500);

    return () => clearInterval(interval);
  }, [isOpen, fetchJobs]);

  const handleCancelJob = async (jobId: string) => {
    try {
      setCancellingId(jobId);
      await tauriApi.cancelBackgroundJob(jobId);
      await fetchJobs();
    } catch (err) {
      console.error('Failed to cancel job:', err);
    } finally {
      setCancellingId(null);
    }
  };

  if (!isOpen) return null;

  const filteredJobs = jobs.filter((job) => {
    if (filter === 'all') return true;
    if (filter === 'running') return job.status === 'running' || job.status === 'queued';
    if (filter === 'completed') return job.status === 'completed';
    if (filter === 'cancelled') return job.status === 'cancelled' || job.status === 'failed';
    return true;
  });

  const runningCount = jobs.filter((j) => j.status === 'running' || j.status === 'queued').length;

  const getJobIcon = (type: string) => {
    switch (type) {
      case 'embedding':
        return <Cpu className="w-4 h-4 text-purple-500" />;
      case 'page_archive':
      case 'archive':
        return <FileDown className="w-4 h-4 text-blue-500" />;
      case 'sync':
        return <Cloud className="w-4 h-4 text-emerald-500" />;
      default:
        return <Layers className="w-4 h-4 text-slate-500" />;
    }
  };

  const formatTime = (ts: number) => {
    return new Date(ts).toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/50 backdrop-blur-xs animate-in fade-in duration-200">
      <div className="flex flex-col w-full max-w-2xl max-h-[85vh] bg-white dark:bg-slate-900 rounded-2xl shadow-2xl border border-slate-200 dark:border-slate-800 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-slate-100 dark:border-slate-800">
          <div className="flex items-center gap-2.5">
            <div className="p-2 rounded-xl bg-blue-50 dark:bg-blue-950/60 text-blue-600 dark:text-blue-400">
              <Activity className="w-5 h-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold text-slate-800 dark:text-slate-100">
                  后台任务与队列中心
                </h2>
                {runningCount > 0 && (
                  <span className="flex items-center gap-1 px-2 py-0.5 text-xs rounded-full bg-blue-100 dark:bg-blue-900/60 text-blue-600 dark:text-blue-300 font-medium animate-pulse">
                    <span className="w-1.5 h-1.5 rounded-full bg-blue-600 dark:bg-blue-400"></span>
                    {runningCount} 个执行中
                  </span>
                )}
              </div>
              <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5">
                监控离线归档下载、语义向量化以及端到端加密同步等后台长耗时任务
              </p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => {
                setLoading(true);
                fetchJobs().finally(() => setLoading(false));
              }}
              disabled={loading}
              className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
              title="刷新任务列表"
            >
              <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button
              onClick={onClose}
              className="p-1.5 rounded-lg text-slate-400 hover:text-slate-600 dark:hover:text-slate-200 hover:bg-slate-100 dark:hover:bg-slate-800 transition cursor-pointer"
            >
              <X className="w-5 h-5" />
            </button>
          </div>
        </div>

        {/* Filter Tabs */}
        <div className="flex items-center gap-2 px-6 py-2.5 bg-slate-50/50 dark:bg-slate-900/50 border-b border-slate-100 dark:border-slate-800">
          {(
            [
              { key: 'all', label: '全部任务', count: jobs.length },
              { key: 'running', label: '运行中', count: runningCount },
              {
                key: 'completed',
                label: '已完成',
                count: jobs.filter((j) => j.status === 'completed').length,
              },
              {
                key: 'cancelled',
                label: '已终止/异常',
                count: jobs.filter((j) => j.status === 'cancelled' || j.status === 'failed').length,
              },
            ] as const
          ).map((tab) => (
            <button
              key={tab.key}
              onClick={() => setFilter(tab.key)}
              className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition cursor-pointer ${
                filter === tab.key
                  ? 'bg-blue-600 text-white shadow-xs'
                  : 'text-slate-600 dark:text-slate-400 hover:bg-slate-100 dark:hover:bg-slate-800'
              }`}
            >
              <span>{tab.label}</span>
              <span
                className={`px-1.5 py-0.2 rounded-full text-[10px] ${
                  filter === tab.key
                    ? 'bg-white/20 text-white'
                    : 'bg-slate-200 dark:bg-slate-800 text-slate-500 dark:text-slate-400'
                }`}
              >
                {tab.count}
              </span>
            </button>
          ))}
        </div>

        {/* Jobs List */}
        <div className="flex-1 overflow-y-auto p-6 space-y-3">
          {filteredJobs.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center text-slate-400">
              <Activity className="w-12 h-12 stroke-[1.2] mb-3 opacity-40 text-slate-400" />
              <p className="text-sm font-medium">当前无匹配的任务记录</p>
              <p className="text-xs text-slate-500 mt-1">
                当触发长耗时离线归档或批量 Embedding 处理时将在此展示进度
              </p>
            </div>
          ) : (
            filteredJobs.map((job) => {
              const isRunning = job.status === 'running' || job.status === 'queued';
              const isDone = job.status === 'completed';
              const isCancelled = job.status === 'cancelled';
              const isFailed = job.status === 'failed';

              const percent =
                job.progress_total > 0
                  ? Math.min(100, Math.round((job.progress_current / job.progress_total) * 100))
                  : isDone
                  ? 100
                  : 0;

              return (
                <div
                  key={job.id}
                  className="flex flex-col gap-2.5 p-4 rounded-xl border border-slate-200/80 dark:border-slate-800 bg-white dark:bg-slate-850/60 hover:border-slate-300 dark:hover:border-slate-700 transition shadow-xs"
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex items-start gap-3">
                      <div className="p-2 rounded-lg bg-slate-100 dark:bg-slate-800 shrink-0 mt-0.5">
                        {getJobIcon(job.job_type)}
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <h3 className="text-sm font-semibold text-slate-800 dark:text-slate-200">
                            {job.title}
                          </h3>
                          <span className="font-mono text-[10px] text-slate-400 uppercase px-1.5 py-0.5 bg-slate-100 dark:bg-slate-800 rounded">
                            {job.job_type}
                          </span>
                        </div>
                        {job.message && (
                          <p className="text-xs text-slate-500 dark:text-slate-400 mt-1 line-clamp-1">
                            {job.message}
                          </p>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 shrink-0">
                      {isRunning && (
                        <>
                          <span className="flex items-center gap-1 text-xs font-medium text-blue-600 dark:text-blue-400">
                            <RefreshCw className="w-3.5 h-3.5 animate-spin" />
                            {percent}%
                          </span>
                          <button
                            onClick={() => handleCancelJob(job.id)}
                            disabled={cancellingId === job.id}
                            className="flex items-center gap-1 px-2.5 py-1 text-xs text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-950/40 hover:bg-red-100 dark:hover:bg-red-900/60 rounded-lg transition cursor-pointer"
                            title="取消当前任务"
                          >
                            <Ban className="w-3 h-3" />
                            <span>{cancellingId === job.id ? '取消中...' : '取消'}</span>
                          </button>
                        </>
                      )}
                      {isDone && (
                        <span className="flex items-center gap-1 text-xs font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/40 px-2 py-0.5 rounded-md">
                          <CheckCircle2 className="w-3.5 h-3.5" />
                          已完成
                        </span>
                      )}
                      {isCancelled && (
                        <span className="flex items-center gap-1 text-xs font-medium text-slate-500 dark:text-slate-400 bg-slate-100 dark:bg-slate-800 px-2 py-0.5 rounded-md">
                          <Ban className="w-3.5 h-3.5" />
                          已取消
                        </span>
                      )}
                      {isFailed && (
                        <span className="flex items-center gap-1 text-xs font-medium text-amber-600 dark:text-amber-400 bg-amber-50 dark:bg-amber-950/40 px-2 py-0.5 rounded-md">
                          <AlertCircle className="w-3.5 h-3.5" />
                          失败
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Progress Bar */}
                  {job.progress_total > 0 && (
                    <div className="flex flex-col gap-1 mt-1">
                      <div className="w-full h-1.5 rounded-full bg-slate-100 dark:bg-slate-800 overflow-hidden">
                        <div
                          className={`h-full transition-all duration-300 ${
                            isRunning
                              ? 'bg-blue-600 dark:bg-blue-500'
                              : isDone
                              ? 'bg-emerald-500'
                              : 'bg-slate-400'
                          }`}
                          style={{ width: `${percent}%` }}
                        />
                      </div>
                      <div className="flex items-center justify-between text-[11px] text-slate-400">
                        <span>
                          进度: {job.progress_current} / {job.progress_total}
                        </span>
                        <div className="flex items-center gap-2">
                          <span className="flex items-center gap-1">
                            <Clock className="w-3 h-3" />
                            {formatTime(job.updated_at)}
                          </span>
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 bg-slate-50 dark:bg-slate-900 border-t border-slate-100 dark:border-slate-800 text-xs text-slate-500 dark:text-slate-400">
          <span>总任务数: {jobs.length} 项</span>
          <button
            onClick={onClose}
            className="px-4 py-1.5 rounded-lg border border-slate-300 dark:border-slate-700 hover:bg-slate-100 dark:hover:bg-slate-800 text-slate-700 dark:text-slate-200 font-medium transition cursor-pointer"
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  );
};
