import { Routes, Route } from 'react-router-dom';
import { Layout } from '@/components/Layout';
import { Runs } from '@/pages/Runs';
import { RepoList } from '@/pages/RepoList';
import { RunDetail } from '@/pages/RunDetail';
import { Settings } from '@/pages/Settings';

function App() {
  return (
    <Layout>
      <Routes>
        <Route path="/" element={<Runs />} />
        <Route path="/runs" element={<Runs />} />
        <Route path="/repos" element={<RepoList />} />
        <Route path="/repos/:repoId/runs/:runId" element={<RunDetail />} />
        <Route path="/settings" element={<Settings />} />
      </Routes>
    </Layout>
  );
}

export default App;
