import { createRoot } from 'react-dom/client';
// The one picture this deployment stands. A view is installed by importing it, so a package imported here is a surface the
// board offers: the map is this project's, and the shell, the graph, and the footage are not installed at all.
import '@ssccsorg/skene-view-map';
import { Frame } from '@ssccsorg/skene-frame';
import { brand } from './brand';
import './pack';
// The chain is this deployment's own picture of a volume, so it arrives with the pack rather than from the frame.
import './chain';
// The site boundary is a layer the map draws, declared where the deployment is: the frame's map has no idea what a
// boundary is.
import './site';
import './styles.css';

// No StrictMode: its double mount opens the record stream twice, and the first socket has already delivered records by
// the time it is closed, which the second one's arrival list has no way to subtract.
const root = document.getElementById('root');
if (!root) throw new Error('the page has no root to render into');

createRoot(root).render(<Frame brand={brand} />);
