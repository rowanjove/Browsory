import React from 'react';
import { Topbar } from '../../components/Layout/Topbar';
import { VirtualHistoryTable } from '../../components/DataTable/VirtualHistoryTable';
import { HistoryDetailDrawer } from '../../components/DetailPanel/HistoryDetailDrawer';

export const HistoryPage: React.FC = () => {
  return (
    <div className="flex-1 flex flex-col h-full overflow-hidden bg-white dark:bg-slate-900">
      <Topbar />
      <div className="flex-1 flex overflow-hidden relative">
        <VirtualHistoryTable />
        <HistoryDetailDrawer />
      </div>
    </div>
  );
};
