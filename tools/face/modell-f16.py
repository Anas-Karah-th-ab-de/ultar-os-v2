#!/usr/bin/env python3
"""SFace (OpenCV-Zoo, Apache-2.0) auf halbe Größe bringen: Gewichte als float16 speichern, beim Laden per
Cast wieder float32 (tract rechnet weiter in f32). Ergebnis gleichwertig (Ähnlichkeit 0.8874 -> 0.8876).
Quelle: https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/face_recognition_sface/face_recognition_sface_2021dec.onnx
Aufruf: python3 modell-f16.py <ein.onnx> <aus.onnx>   (pip install onnx numpy)"""
import sys, onnx, numpy as np
from onnx import numpy_helper, helper, TensorProto

m = onnx.load(sys.argv[1]); g = m.graph
inits = {i.name for i in g.initializer}
keep = [i for i in g.input if i.name not in inits]   # Gewichte sind (altes Format) auch als Eingänge gelistet
del g.input[:]; g.input.extend(keep)
new, casts = [], []
for t in list(g.initializer):
    a = numpy_helper.to_array(t)
    if a.dtype == np.float32 and a.size >= 256:
        new.append(numpy_helper.from_array(a.astype(np.float16), t.name + '_f16'))
        casts.append(helper.make_node('Cast', [t.name + '_f16'], [t.name], to=TensorProto.FLOAT, name='cast_' + t.name))
    else:
        new.append(t)
del g.initializer[:]; g.initializer.extend(new)
nodes = casts + list(g.node); del g.node[:]; g.node.extend(nodes)
onnx.checker.check_model(m); onnx.save(m, sys.argv[2])
