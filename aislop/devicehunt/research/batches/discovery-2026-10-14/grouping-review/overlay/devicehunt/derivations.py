"""Versioned offline recipes; saved evidence is never a guessed compatible recipe."""
from __future__ import annotations
import ast
import inspect
import json
import platform
import textwrap
import uuid
from functools import lru_cache
from pathlib import Path
from .artifacts import canonical, digest

# Recipe identity binds only the code a recipe depends on: its own implementation function(s)
# (hashed from the module's AST, so comments/whitespace/unrelated edits do not matter), an explicit
# per-recipe code version to bump for behaviour changes the AST cannot show (e.g. a changed
# dependency's semantics), and the external helpers it calls. Edits elsewhere in this module
# (other recipes, register_saved, run bookkeeping) leave recipe ids stable so saved runs stay reusable.
RECIPE_CODE_VERSION = {'page-text-flatten': '1', 'xml-locations': '1'}
RECIPE_FUNCTIONS = {'page-text-flatten': ('_page_text_flatten',), 'xml-locations': ('_xml_locations',)}
RECIPE_EXTERNAL_HELPERS = (canonical,)  # used to serialise every output


@lru_cache(maxsize=8)
def _function_hashes(module_source):
    tree = ast.parse(module_source)
    return {node.name: digest(ast.dump(node).encode()) for node in tree.body
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))}


def recipe_code_sha256(name, module_source=None):
    """Deterministic hash of one recipe's own code; `module_source` lets tests probe edits."""
    if module_source is None:
        module_source = Path(__file__).read_text(encoding='utf-8')
    functions = _function_hashes(module_source)
    own = {fn: functions[fn] for fn in RECIPE_FUNCTIONS[name]}
    helpers = {h.__qualname__: digest(ast.dump(ast.parse(textwrap.dedent(inspect.getsource(h)))).encode())
               for h in RECIPE_EXTERNAL_HELPERS}
    return digest(canonical(dict(recipe=name, code_version=RECIPE_CODE_VERSION[name], functions=own, helpers=helpers)))


def _page_text_flatten(data, configuration):
    source = json.loads(data)
    pages = source['pages']
    if not isinstance(pages, list):
        raise ValueError('pages must be a list')
    # Preserve order, duplicates and empty page text.
    return {'schema_version': 1, 'pages': pages, 'text': configuration.get('separator', '\n').join(page['text'] for page in pages)}


def _xml_locations(data):
    import xml.etree.ElementTree as ET
    root = ET.fromstring(data)
    return {'schema_version': 1, 'locations': [node.text or '' for node in root.iter() if node.tag.rsplit('}', 1)[-1] == 'loc']}


class Derivations:
    def __init__(self, archive):
        self.archive = archive

    def recipe(self, name, version, configuration=None):
        if name not in ('page-text-flatten', 'xml-locations'):
            raise ValueError('unsupported local recipe')
        if version != '1':
            raise ValueError('unsupported recipe version')
        configuration = configuration or {}
        allowed = {'separator'} if name == 'page-text-flatten' else set()
        if set(configuration) - allowed or ('separator' in configuration and not isinstance(configuration['separator'], str)):
            raise ValueError('unsupported recipe configuration')
        payload = dict(name=name,version=version,configuration=configuration or {},code_sha256=recipe_code_sha256(name),dependencies={'python':platform.python_version()},output_schema_version=1)
        identifier = 'recipe-' + digest(canonical(payload))
        self.archive.receipt('recipe',payload,identifier)
        return identifier

    def register_saved(self, inputs, output_sha256, method, locator=None):
        inputs = self._inputs(inputs)
        self.archive.resolve(output_sha256)
        payload = dict(inputs=inputs,outputs=[{'role':'saved-output','sha256':output_sha256}],method=method,recipe_id=None,tool_version=None,code_sha256=None,configuration=None,extracted_at=None,historical_status=None,output_schema_version=None,metadata_status='unknown',locator=locator)
        identifier = 'saved-derivation-' + digest(canonical(payload))
        self.archive.receipt('saved-derivation',payload,identifier)
        return identifier

    def _inputs(self, inputs):
        result = []
        for order, item in enumerate(inputs):
            if not isinstance(item, dict) or set(item) - {'role','sha256'} or not isinstance(item.get('role'),str):
                raise ValueError('input requires role and sha256')
            self.archive.resolve(item['sha256'])
            result.append(dict(role=item['role'],sha256=item['sha256'],order=order))
        if not result:
            raise ValueError('at least one input required')
        return result

    def run(self, recipe_id, inputs, consumer=None, force=False):
        recipe = self.archive.get_receipt(recipe_id)
        if recipe['kind'] != 'recipe':
            raise ValueError('not a recipe')
        definition = recipe['payload']
        current = dict(name=definition['name'],version='1',configuration=definition['configuration'],code_sha256=recipe_code_sha256(definition['name']),dependencies={'python':platform.python_version()},output_schema_version=1)
        if definition != current:
            raise ValueError('recipe does not match installed implementation/runtime')
        inputs = self._inputs(inputs)
        if not force:
            for receipt in self.archive.derivation_candidates(recipe_id,inputs):
                p = receipt['payload']
                if p.get('status')=='success' and p['recipe_id']==recipe_id and p['inputs']==inputs:
                    try:
                        for output in p['outputs']:
                            self.archive.resolve(output['sha256'])
                    except ValueError:
                        continue
                    self._consumer(receipt['id'],consumer)
                    return receipt
        run_id = 'run-' + str(uuid.uuid4())
        payload = dict(recipe_id=recipe_id,inputs=inputs,status='failed',outputs=[],error=None)
        try:
            if len(inputs)!=1:
                raise ValueError('local recipes require one input')
            data = self.archive.resolve(inputs[0]['sha256']).read_bytes()
            definition = recipe['payload']
            if definition['name']=='page-text-flatten':
                output = _page_text_flatten(data,definition['configuration'])
            elif definition['name']=='xml-locations':
                output = _xml_locations(data)
            else:
                raise ValueError('unsupported recipe')
            sha = self.archive.store_bytes(canonical(output))
            payload.update(status='success',outputs=[{'role':'result','sha256':sha}])
        except Exception as exc:
            payload['error'] = {'type':type(exc).__name__,'message':str(exc)}
            self.archive.receipt('derivation-run',payload,run_id)
            raise
        self.archive.receipt('derivation-run',payload,run_id)
        self._consumer(run_id,consumer)
        return self.archive.get_receipt(run_id)

    def _consumer(self, run_id, consumer):
        if consumer is not None:
            payload = dict(run_id=run_id,consumer=consumer)
            self.archive.receipt('derivation-consumer',payload,'consumer-' + digest(canonical(payload)))


# Convenient functional wrappers for callers that do not retain a manager.
def register_recipe(archive, name, version='1', configuration=None):
    return Derivations(archive).recipe(name,version,configuration)


def run_recipe(archive, recipe_id, inputs, consumer=None, force=False):
    return Derivations(archive).run(recipe_id,inputs,consumer,force)
