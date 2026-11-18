import { createRoot } from 'react-dom/client';
// The one picture this deployment stands. A view is installed by importing it, so a package imported here is a surface the
// board offers. The frame's own set is three: the map, the shell, and the footage, which is the set another deployment of
// this frame installs. The application's own readings, the records and the chain, arrive with the pack.
import '@ssccsorg/skene-view-map';
import '@ssccsorg/skene-view-shell';
import '@ssccsorg/skene-view-media';
import { Frame, NarrowNotice, offTheShelf } from '@ssccsorg/skene-frame';
import { brand } from './brand';
import './pack';
// The chain is this deployment's own picture of a volume, so it arrives with the pack rather than from the frame.
import './chain';
// The picked record's detail is a panel of the frame, registered where it is drawn.
import './evidence';
// The site boundary is a layer the map draws, declared where the deployment is: the frame's map has no idea what a
// boundary is.
import './site';
import './styles.css';

// No StrictMode: its double mount opens the record stream twice, and the first socket has already delivered records by
// the time it is closed, which the second one's arrival list has no way to subtract.
const root = document.getElementById('root');
if (!root) throw new Error('the page has no root to render into');

createRoot(root).render(
  <>
    <Frame brand={brand} host={offTheShelf} />
    <NarrowNotice title="A window is required." dismissible>
      The columns, the split, and the map are sized for one; on a phone this board is readable and not operable.
    </NarrowNotice>
  </>,
);
