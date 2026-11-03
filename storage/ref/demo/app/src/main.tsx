import { createRoot } from 'react-dom/client';
// The pictures this deployment draws, each a package that installs itself.
import '@ssccsorg/skene-view-media';
import '@ssccsorg/skene-view-map';
import '@ssccsorg/skene-view-shell';
import '@ssccsorg/skene-view-graph';
import { Frame } from '@ssccsorg/skene-frame';
import { brand } from './brand';
import './pack';
import './styles.css';

// No StrictMode: its double mount opens the record stream twice, and the first socket has already delivered records by
// the time it is closed, which the second one's arrival list has no way to subtract.
const root = document.getElementById('root');
if (!root) throw new Error('the page has no root to render into');

createRoot(root).render(<Frame brand={brand} />);
