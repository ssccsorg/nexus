// The site boundary the map draws, taken from the place's own ground.
//
// A map draws a place but not its areas: the frame leaves "this rectangle is the subject" to the deployment that means
// it, and asks through the overlay register. This is that claim, and it is drawn from the box the place already
// declares, so the boundary cannot end up around ground the units do not stand on.
//
// What it asserts is narrow: the declared site is the declared box. It says nothing about where a meter physically is,
// because the records carry no position. The site label comes from the place's own store, which the importer fills from
// the `site` field the measured records carry, so the map and the records name the same site.

import { collectionOf, polygonOf, registerOverlays } from '@ssccsorg/skene-view-map';

registerOverlays('site', (place) => {
  const [x, y, w, h] = place.view;
  const label = place.store?.label;
  const boundary = polygonOf(place, { x, y, w, h }, label ? { label } : {});
  if (!boundary) return [];
  // Dashed, because a boundary is a declaration about a place and not a thing standing on it.
  return [{ id: 'site', kind: 'pointing', data: collectionOf([boundary]) }];
});
