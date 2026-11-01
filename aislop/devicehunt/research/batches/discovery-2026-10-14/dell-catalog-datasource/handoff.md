# Dell bulk firmware catalog — stopped on robots policy

Official primary-source discovery: Dell PowerEdge catalog-links article
https://www.dell.com/support/kbdoc/en-us/000132986/dell-emc-catalog-links-for-poweredge-servers?lang=en
publishes the enterprise Catalog.xml.gz endpoint. This is a distinct broad source
from LVFS, evaluated for system/component reference metadata only.

One request to https://downloads.dell.com/robots.txt returned200;
SHA256 bc2685b272d681dcf97518aad040ca63ab0527f98cdcbb75e4f9aa733386aebd.
Robots allows /manuals and /topicspdf but disallows the remaining host paths.
/catalog/Catalog.xml.gz was NOT requested. No retry, mirror, alternate route,
firmware payload or system change. Unavailable, not an empty hardware catalog.
