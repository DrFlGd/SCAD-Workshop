# mitcad-asm: the ASM B-rep reader

`core/asm` reads the ASM B-rep data that `.f3d` body blobs, the B-rep
records of `.ipt` parts and the solids of `.dwg` drawings hold, and
converts it to a neutral B-rep model in millimetres:

| Module | Contents |
|---|---|
| `asm` | The ASM binary format (and the text form drawings keep): header, tokens, records, geometry, the history of `.smbh` blobs; a writer for tests. |
| `brep` | The neutral B-rep model with checks. |
| `convert` | ASM bodies to the neutral model, at any history state. |
| `flat` | A neutral body as plain arrays for the C++ builder. |
| `ids` | Maps and sets keyed by record and entity indices. |
| `history_states` | Which states of a blob's history hold the same bodies (mitcad#675). |
| `testdata` | Small ASM bodies made by the writer, for tests here, in the readers and in the C++ bridge tests. |
| `read_blob` | One body blob parsed and its top-level bodies converted. |

The format is described in [ASM_FORMAT.md](../f3d/ASM_FORMAT.md) and
[EDGE_ORDER.md](../f3d/EDGE_ORDER.md). Until mitcad#776 this was part of
`mitcad-f3d`, which re-exports it under the same paths
(`mitcad_f3d::asm`, `mitcad_f3d::convert`, ...).
