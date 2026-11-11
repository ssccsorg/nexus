// The footage, served from this deployment's own origin.
//
// The board this deployment carries was written by another deployment, so it names the camera views the same way:
// `/media/...`, which a dev server proxies and a static deployment does not. The proxy itself is the frame's, because
// what it answers is the media view's constraint: a cross-origin video taints the canvas it is sampled into.

import { mediaProxy } from '@ssccsorg/skene-frame/tools/media-proxy.mjs';

const FOOTAGE = 'https://pub-dc90fe2d3f7f431897f3c088a1130e0c.r2.dev';

const proxy = mediaProxy(FOOTAGE);

export const onRequest = proxy.onRequest;
