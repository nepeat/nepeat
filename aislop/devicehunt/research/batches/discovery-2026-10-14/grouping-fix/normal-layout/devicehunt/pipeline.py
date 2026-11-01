"""Shared, explicit source dispatcher for individual imports and offline builds."""
from . import importers, onie, fcc, edgecore, ntra, ul, tec, marketplace, oracle_evidence
from . import grantees
from . import icasa
from . import bahrain
from . import broadcom
from . import anatel
from . import anatel_accton_portfolio
from . import ised
from . import nigeria
from . import ghana
from . import public_registers
from . import japan_mic
from . import ul_platforms
from . import ul_celestica
from . import ul_amazon
from . import meta_networking
from . import ocp
from . import ntra_register
from . import ntra_historical
from . import oracle_components
from . import oracle_ecodesign
from . import ncc
from . import bsmi
from . import bsmi_additional
from . import taiwan_targets
from . import ieee
from . import pci
from . import fcc_index
from . import fcc_company
from . import fcc_sitemaps
from . import fcc_reviewed_index
from . import rra_reviewed
from . import rra
from . import rra_manufacturer_lists
from . import rra_sun
from . import documents
from . import monitron
from . import celestica_networking
from . import celestica_catalog
from . import fcc_details
from . import fcc_gap_probes
from . import fcc_cover_models
from . import fcc_sporton
from . import ocnos
from . import picos
from . import energystar
from . import energystar_archive
from . import energystar_datacenter
from . import tec_register
from . import cb_certificates
from . import panorama
from . import reference_documents
from . import snowcone
from . import inventec
from . import inspur_support, lineage

IMPORTERS = {
    'archive-release': lineage.import_bundle,
    'devicecode': importers.devicecode,
    'sonic': importers.sonic,
    'onl': importers.onl,
    'curated': importers.curated,
    'inventec-server-index': inventec.import_page,
    'inspur-support': inspur_support.import_bundle,
    'onie': onie.import_bundle,
    'fcc': fcc.import_manifest,
    'fcc-grantees': grantees.import_bundle,
    'icasa': icasa.import_bundle,
    'bahrain': bahrain.import_bundle,
    'broadcom': broadcom.import_bundle,
    'anatel': anatel.import_bundle,
    'anatel-accton-portfolio': anatel_accton_portfolio.import_bundle,
    'ised': ised.import_capture,
    'nigeria-ncc': nigeria.import_bundle,
    'ghana-nca': ghana.import_bundle,
    'public-register': public_registers.import_bundle,
    'japan-mic': japan_mic.import_bundle,
    'edgecore': edgecore.import_bundle,
    'edgecore-eol': edgecore.import_notice,
    'edgecore-notices': edgecore.import_notices,
    'ntra': ntra.import_pdf,
    'ntra-saved-targets': ntra.import_saved,
    'ntra-register': ntra_register.import_bundle,
    'ntra-historical': ntra_historical.import_bundle,
    'oracle-components': oracle_components.import_bundle,
    'oracle-ecodesign': oracle_ecodesign.import_bundle,
    'ncc': ncc.import_bundle,
    'bsmi': bsmi.import_bundle,
    'bsmi-additional': bsmi_additional.import_bundle,
    'taiwan-targets': taiwan_targets.import_bundle,
    'ieee': ieee.import_bundle,
    'pci-ids': pci.import_bundle,
    'fcc-index': fcc_index.import_bundle,
    'fcc-company': fcc_company.import_bundle,
    'fcc-sitemaps': fcc_sitemaps.import_bundle,
    'fcc-reviewed-index': fcc_reviewed_index.import_bundle,
    'rra-reviewed': rra_reviewed.import_bundle,
    'rra': rra.import_bundle,
    'rra-manufacturer-lists': rra_manufacturer_lists.import_bundle,
    'rra-sun-portfolio': rra_sun.import_bundle,
    'fcc-documents': documents.import_bundle,
    'aws-monitron': monitron.import_bundle,
    'celestica-catalog-pdf': celestica_catalog.import_bundle,
    'celestica-networking': celestica_networking.import_bundle,
    'fcc-details': fcc_details.import_bundle,
    'fcc-gap-probes': fcc_gap_probes.import_bundle,
    'fcc-cover-models': fcc_cover_models.import_capture,
    'fcc-sporton-covers': fcc_sporton.import_capture,
    'ocnos-hcl': ocnos.import_bundle,
    'picos-hardware': picos.import_bundle,
    'energystar-servers': energystar.import_bundle,
    'energystar-server-archive': energystar_archive.import_bundle,
    'energystar-datacenter': energystar_datacenter.import_bundle,
    'tec-register': tec_register.import_table,
    'cb-certificates': cb_certificates.import_bundle,
    'aws-panorama': panorama.import_bundle,
    'reference-documents': reference_documents.import_bundle,
    'aws-snowcone': snowcone.import_bundle,
    'ul-capture': ul.import_capture,
    'ul-platforms': ul_platforms.import_capture,
    'ul-celestica-platforms': ul_celestica.import_capture,
    'ul-amazon-platforms': ul_amazon.import_capture,
    'meta-networking': meta_networking.import_bundle,
    'ocp-capture': ocp.import_capture,
    'tec': tec.import_table,
    'ecrunch': marketplace.import_ecrunch,
}
SOURCE_NAMES = sorted(set(IMPORTERS) | set(importers.ORACLE_URLS) |
                      set(importers.HARDWARE_INDEXES) | set(oracle_evidence.URLS))


def import_source(c, source, path):
    if source in oracle_evidence.URLS:
        return oracle_evidence.import_page(c, path, source)
    if source in importers.HARDWARE_INDEXES:
        return importers.hardware_index(c, path, source)
    if source in importers.ORACLE_URLS:
        return importers.oracle(c, path, source)
    if source not in IMPORTERS:
        raise ValueError('Unknown source importer: ' + str(source))
    return IMPORTERS[source](c, path)
