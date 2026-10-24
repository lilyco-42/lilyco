"""Studio / CLI-Anything / Blender reference workflow safety tests (no models needed)."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
import studio_mcp as studio


class StudioTests(unittest.TestCase):
    def test_lyco_market_has_blender_and_local_workflow(self):
        # cli-hub optional, lookup network must never determine whether bundled skills exist
        with patch.object(studio.shutil,'which',return_value=None):
            data=studio.skill_search('三视图 blender 3d')
        names=[s['id'] for s in data['matched']]
        self.assertIn('cli-anything.blender',names)
        self.assertIn('lyco.blender.pipeline',names)
        self.assertIn('lyco.3d.reference-stage',names)
        self.assertEqual(data['cli_anything']['status'],'unavailable')

    def test_preflight_does_not_install_software(self):
        with patch.object(studio,'blender_executable',return_value=None),patch.object(studio.shutil,'which',return_value=None):
            result=studio.preflight()
        self.assertFalse(result['blender']['installed'])
        self.assertFalse(result['full_reconstruction_available'])
        self.assertEqual(result['next'],'requires_approved_blender_install')

    def test_image_inputs_are_real_and_bounded(self):
        with tempfile.TemporaryDirectory() as td:
            path=Path(td)/'image.png'
            path.write_bytes(b'\x89PNG\r\n\x1a\n' + b'example')
            self.assertEqual(studio._input_image(str(path)),path.resolve())
            path.write_bytes(b'bad!')
            with self.assertRaises(studio.WorkflowError):
                studio._input_image(str(path))
            with self.assertRaises(studio.WorkflowError):
                studio._input_image(str(Path(td)/'nonexistent.png'))

    def test_workflow_refuses_existing_project_or_wrong_env(self):
        with tempfile.TemporaryDirectory() as td:
            root=Path(td)
            for key in ('front','side','back'):
                (root/(key+'.png')).write_bytes(b'\x89PNG\r\n\x1a\n'+b'fake')
            params={k:str(root/(k+'.png')) for k in ('front','side','back')}
            with patch.dict(os.environ,{'LILYCO_AUTOMATION_ROOT':str(root)}):
                (root/'three-view-project').mkdir()
                with self.assertRaises(studio.WorkflowError):
                    studio.create_reference_stage(params)
                (root/'three-view-project').rmdir()
                params['project']='../../unsafe'
                with self.assertRaises(studio.WorkflowError):
                    studio.create_reference_stage(params)

    def test_tools_are_schema_stable(self):
        methods=[x['name'] for x in studio.TOOLS]
        self.assertEqual(methods,['lyco_find_skills','lyco_environment_preflight','lyco_create_3d_reference'])
        r=studio.mcp_message({'jsonrpc':'2.0','id':1,'method':'tools/list','params':{}})
        self.assertEqual(len(r['result']['tools']),3)
        bad=studio.mcp_message({'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'unknown','arguments':{}}})
        self.assertTrue(bad['result']['isError'])

    def test_output_root_is_explicit(self):
        with patch.dict(os.environ,{'LILYCO_AUTOMATION_ROOT':''}):
            with self.assertRaises(studio.WorkflowError):
                studio._job_workspace()


if __name__=='__main__':
    unittest.main()

