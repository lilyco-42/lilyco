"""Blender background worker for Lilyco 3-view drafting.

This script is bundled and reviewed. It must NEVER evaluate remote Skill source
code or accept arbitrary Blender Python expressions from agent arguments.
"""
import bpy
import json
import math
from mathutils import Vector
from pathlib import Path
import sys

argv = sys.argv
if '--' not in argv:
    raise SystemExit("expected project manifest after --")
manifest = json.loads(Path(argv[argv.index('--')+1]).read_text(encoding='utf-8'))
paths = manifest['views']
out = Path(manifest['output'])
out.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)

# A starter proxy ONLY. No claim that this matches the shape described by images.
bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 0))
proxy = bpy.context.object
proxy.name = "DRAFT_PROXY_replace_with_reconstructed_mesh"
proxy.scale = (0.55, 0.35, 0.95)
bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
mod = proxy.modifiers.new('Preview softened edges', 'BEVEL')
mod.width = 0.08
mod.segments = 3
proxy.modifiers.new('Weighted preview normals', 'WEIGHTED_NORMAL')
material = bpy.data.materials.new('Draft proxy - not final geometry')
material.diffuse_color = (0.35, 0.64, 0.88, 1)
proxy.data.materials.append(material)

# The image planes are Blender empties so they do not interfere with mesh exports.
# Blender front is looking along negative Y; side looks along negative X.
orient = {
    'front': ((0, 0.85, 0), (math.pi/2, 0, 0)),
    'side': ((0.85, 0, 0), (math.pi/2, 0, math.pi/2)),
    'back': ((0, -0.85, 0), (math.pi/2, 0, math.pi)),
}
for key in ('front', 'side', 'back'):
    image = bpy.data.images.load(paths[key], check_existing=True)
    empty = bpy.data.objects.new(f'REFERENCE_{key.upper()}_NO_MESH', None)
    bpy.context.collection.objects.link(empty)
    empty.empty_display_type = 'IMAGE'
    empty.data = image
    empty.empty_display_size = 2.0
    empty.location = orient[key][0]
    empty.rotation_euler = orient[key][1]
    empty.show_in_front = True
    empty.hide_render = True

world = bpy.context.scene.world
if world:
    world.color = (0.08, 0.08, 0.08)

# Camera preview of the draft proxy, not 3D reconstruction.
bpy.ops.object.camera_add(location=(3.2, -4.6, 2.6))
camera = bpy.context.object
camera.name='Preview Camera'
direction = Vector((0,0,0)) - camera.location
camera.rotation_euler=direction.to_track_quat('-Z','Y').to_euler()
camera.data.type='ORTHO'
camera.data.ortho_scale=3.7
bpy.context.scene.camera=camera
bpy.ops.object.light_add(type='AREA', location=(1.5,-1.8,3))
lamp=bpy.context.object
lamp.data.energy=550
lamp.data.shape='DISK'
lamp.data.size=4
scene=bpy.context.scene
scene.render.engine='BLENDER_EEVEE'
scene.render.resolution_x=640
scene.render.resolution_y=640
scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG'
scene.render.filepath=str(out/'draft_preview.png')
scene.camera.data.lens=50
# Pack all supplied reference images into the Blend file for portable delivery.
bpy.ops.file.pack_all()
bpy.ops.wm.save_as_mainfile(filepath=str(out/'reference_stage.blend'))

# Separate draft proxy GLB for testing downstream export, not a deliverable high-fidelity mesh.
bpy.ops.export_scene.gltf(filepath=str(out/'DRAFT_PROXY.glb'),export_format='GLB',
                          use_selection=True, export_yup=True) if False else None
bpy.ops.render.render(write_still=True)
result = {
    'status':'reference_stage_ready',
    'quality':'draft_only',
    'blend': str(out/'reference_stage.blend'),
    'preview': str(out/'draft_preview.png'),
    'proxy_vertices': len(proxy.data.vertices),
    'reference_views': len(paths),
    'message':'Created editable Blender reference stage; high-quality 3D reconstruction NOT yet performed.'
}
(out/'stage_result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print('LYCO_STAGE_RESULT='+json.dumps(result,ensure_ascii=False))

