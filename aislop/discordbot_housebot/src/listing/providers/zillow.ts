import { identifyUrl } from '../url';
import type { ListingSource, Snapshot } from '../types';
import {
  addressFromMeta,
  finalize,
  mergeSnapshot,
  snapshotFromJsonLd,
  snapshotFromScan,
} from './common';

export const zillowSource: ListingSource = {
  provider: 'zillow',

  matches(url: URL) {
    return /(^|\.)zillow\.com$/i.test(url.hostname);
  },

  identify(url: URL) {
    const id = identifyUrl(url.toString());
    if (!id || id.provider !== 'zillow') return null;
    return { listingId: id.listingId, canonicalUrl: id.canonicalUrl };
  },

  parse(html, canonicalUrl, listingId, fetchedAt): Snapshot | null {
    const merged = mergeSnapshot(
      snapshotFromJsonLd(html, 'zillow', listingId, canonicalUrl, fetchedAt),
      snapshotFromScan(html),
      addressFromMeta(html),
    );
    return finalize(merged, 'zillow', listingId, canonicalUrl, fetchedAt);
  },
};
