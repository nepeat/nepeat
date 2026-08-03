import { identifyUrl } from '../url';
import type { ListingSource, Snapshot } from '../types';
import {
  addressFromMeta,
  finalize,
  mergeSnapshot,
  snapshotFromJsonLd,
  snapshotFromScan,
} from './common';

export const redfinSource: ListingSource = {
  provider: 'redfin',

  matches(url: URL) {
    return /(^|\.)redfin\.com$/i.test(url.hostname);
  },

  identify(url: URL) {
    const id = identifyUrl(url.toString());
    if (!id || id.provider !== 'redfin') return null;
    return { listingId: id.listingId, canonicalUrl: id.canonicalUrl };
  },

  parse(html, canonicalUrl, listingId, fetchedAt): Snapshot | null {
    const merged = mergeSnapshot(
      snapshotFromJsonLd(html, 'redfin', listingId, canonicalUrl, fetchedAt),
      snapshotFromScan(html),
      addressFromMeta(html),
    );
    return finalize(merged, 'redfin', listingId, canonicalUrl, fetchedAt);
  },
};
