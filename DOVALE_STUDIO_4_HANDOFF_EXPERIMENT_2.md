# DO VALE STUDIO 4 — HANDOFF
## GPU Experiment 2 — Estado actual para continuar en un chat nuevo

> Este documento es el contexto de continuidad de Do Vale Studio 4.
> NO empezar desde cero.
> NO rehacer pasos ya validados.
> Continuar exactamente desde el fallo visual descrito al final.

---

# 1. PROYECTO

**Nombre:** Do Vale Studio 4

**Objetivo:** editor de vídeo nativo de escritorio, profesional y de alto rendimiento.

**Plataformas objetivo:**
- Windows
- macOS más adelante

**Arquitectura general prevista:**
- Rust
- FFmpeg para media/decode
- D3D11VA en Windows para hardware decode
- wgpu / DX12 para render GPU
- egui/eframe para UI del editor
- egui NO debe actuar como compositor de vídeo
- viewport de vídeo dedicado y GPU-first
- evitar GPU→CPU→GPU en reproducción normal

---

# 2. HARDWARE DE PRUEBAS

**Sistema:**
- Windows 10 22H2
- Build 19045

**CPU:**
- AMD Ryzen 7 1700X
- 8C / 16T

**RAM:**
- 32 GB

**GPU:**
- AMD Radeon RX 580
- 8 GB VRAM

**Monitores:**
- 2 × 1080p ~60 Hz

---

# 3. LECCIÓN PRINCIPAL DE DO VALE STUDIO 3

Studio 3 ya conseguía D3D11VA, pero después hacía:

```text
D3D11 HW frame
→ av_hwframe_transfer_data
→ CPU
→ swscale
→ RGBA
→ upload a GPU
```

Mediciones aproximadas de Studio 3 en 4K HEVC:

- GPU→CPU hw transfer: ~8.4 ms/frame
- swscale: ~3.4 ms/frame
- RGBA copy: ~1.9 ms/frame
- pull total: ~13.5 ms/frame antes de upload/UI
- scrub seek+pull: ~47 ms/update

Cuello de botella principal:
**GPU→CPU→GPU obligatorio.**

Studio 4 busca un pipeline **GPU-resident**, no necesariamente zero-copy.

---

# 4. WORKSPACE

Proyecto local:

```text
C:\proyectos-cursor\do-vale-studio-4
```

Workspace principal con crates:

- dvs-app
- dvs-core
- dvs-ui
- dvs-media
- dvs-decoder
- dvs-playback
- dvs-gpu
- dvs-render
- dvs-audio
- dvs-cache
- dvs-project
- dvs-export
- dvs-ai

Experimentos aislados:

- tests/gpu_probe
- tests/gpu_nv12
- tests/gpu_d3d11_interop

IMPORTANTE:
Hasta ahora el trabajo del Experimento 2 se mantiene fuera de los crates de producción.

Auditoría:
`git status --short crates/` vacío.

**No se han modificado crates de producción.**

---

# 5. DOCUMENTACIÓN EXISTENTE

Entre otros:

- PROJECT_CONTEXT.md
- ARCHITECTURE.md
- PERFORMANCE.md
- ROADMAP.md
- DEVELOPMENT.md
- .cursor/rules/do-vale-studio.mdc
- docs/architecture/ARCHITECTURE_REVIEW.md
- docs/gpu/GPU_ARCHITECTURE_SPIKE.md
- docs/gpu/GPU_PROBE_RESULTS.md
- docs/gpu/GPU_EXPERIMENT_1.md
- docs/gpu/GPU_EXPERIMENT_2.md
- docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md
- docs/fixtures/README.md

---

# 6. FFMPEG

Instalado:

```text
FFmpeg 9.0.1-full_build-www.gyan.dev
```

Build shared con headers y `.lib`.

FFmpeg tiene soporte confirmado para:
- d3d11va
- d3d12va
- dxva2
- Vulkan
- AMF
- etc.

`ffmpeg-sys-next`:
- 8.1.0

wgpu:
- 27.0.1

wgpu-hal:
- 27.0.4

wgpu-core:
- 27.0.3

---

# 7. FIXTURES

Fixture principal del Experimento 2:

```text
docs/fixtures/test_4k_hevc_8bit30.mp4
```

Propiedades:

- HEVC
- Main
- 3840 × 2160
- yuv420p
- 8-bit
- 30000/1001 FPS (~29.97)

El decoder D3D11VA reserva textura:

```text
3840 × 2176
```

Visible real:
```text
3840 × 2160
```

Hay 16 líneas extra de padding/alignment.

Existe también un fixture anterior de 10-bit ~60 FPS para pruebas futuras P010.

NO usarlo para el Experimento 2 actual.

---

# 8. EXPERIMENTO 0

Completado.

Resultados principales:

- RX 580 abre Vulkan
- RX 580 abre DX12
- OpenGL no útil para NV12/P010
- DX12/Vulkan pueden crear texturas NV12/P010
- NV12 plane views:
  - Y = R8Unorm
  - UV = Rg8Unorm
- P010:
  - Y = R16Unorm
  - UV = Rg16Unorm
- max texture dimension = 16384
- external D3D11VA import todavía no estaba validado en aquel momento
- DX12 elegido como backend primario

---

# 9. EXPERIMENTO 1

Completado.

Objetivo:
Render sintético NV12 → WGSL → RGB.

Resultados:

- patrón YUV sintético 1920×1080
- WGSL BT.709 limited-range
- GPU scale → 1280×720
- swapchain BGRA8 sRGB
- ~60 FPS presentation
- no CPU YUV→RGB

Hallazgo importante:

wgpu 27 + RX580 rechaza `COPY_DST` directamente sobre `TextureFormat::NV12`.

Por eso Experimento 1 usó texturas separadas:

- R8Unorm Y
- Rg8Unorm UV

El acceso real multi-planar del decoder seguía pendiente.

---

# 10. OBJETIVO DE GPU EXPERIMENT 2

Pipeline deseado:

```text
HEVC
→ FFmpeg D3D11VA
→ AV_PIX_FMT_D3D11
→ ID3D11Texture2D NV12
→ GPU CopySubresourceRegion
→ D3D11 NV12 shareable texture
→ NT shared HANDLE
→ ID3D12Resource
→ GPU fence synchronization
→ wgpu-hal DX12
→ NV12 Plane0 / Plane1
→ WGSL YUV→RGB
→ render / present
```

Terminología:

**GPU-resident pipeline**

NO llamarlo `zero-copy`, porque existe una copia GPU→GPU explícita.

---

# 11. RESTRICCIONES ABSOLUTAS DEL EXPERIMENTO

En el camino normal NO usar:

- av_hwframe_transfer_data
- swscale
- CPU YUV→RGB
- CPU RGBA
- Map() para sacar píxeles
- ReadFromSubresource para frame playback
- GPU→CPU→GPU
- software decode como fallback silencioso
- texturas sintéticas para fingir que el recurso real llegó a wgpu

Si el camino GPU real falla:
**parar y analizar, no inventar fallback.**

---

# 12. GPU EXPERIMENT 2 — RESULTADOS 1–32

Todo esto está VALIDADO.

## FFmpeg / decoder

- input abierto con libavformat
- video stream index = 0
- codec_id = 172 = HEVC
- AVCodecContext creado
- decoder HEVC abierto
- D3D11VA AVHWDeviceContext creado
- D3D11VA attached
- get_format callback activo

Candidates observados:

```text
dxva2_vld
d3d11va_vld
d3d11
d3d12
vaapi
cuda
vulkan
yuv420p
```

Seleccionado:

```text
AV_PIX_FMT_D3D11 = 171
```

Primer frame:

- format = d3d11
- 3840×2160
- PTS = 0
- real D3D11 hardware frame

---

# 13. TEXTURA D3D11 DEL DECODER

Extraído de `AVFrame`:

- `frame.data[0]` → ID3D11Texture2D
- `frame.data[1]` → array slice

Ejemplo observado:

```text
array slice = 19
```

`hw_frames_ctx` presente.

Formatos:

```text
hw format = d3d11
sw format = nv12
```

GetDesc de textura decoder:

```text
Width: 3840
Height: 2176
MipLevels: 1
ArraySize: 20
Format: DXGI_FORMAT_NV12
Usage: D3D11_USAGE_DEFAULT
BindFlags: D3D11_BIND_DECODER
CPUAccessFlags: 0
MiscFlags: 0
```

Conclusión:

**La textura que crea FFmpeg NO es directamente shareable.**

---

# 14. TEXTURA D3D11 SHAREABLE

Creada con éxito:

```text
Width: 3840
Height: 2176
ArraySize: 1
Format: DXGI_FORMAT_NV12
Usage: D3D11_USAGE_DEFAULT
BindFlags: D3D11_BIND_SHADER_RESOURCE
CPUAccessFlags: 0
MiscFlags:
  D3D11_RESOURCE_MISC_SHARED_NTHANDLE
  D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX
```

MiscFlags observado:

```text
0x900
```

---

# 15. GPU COPY

`CopySubresourceRegion` validado.

Ejemplo:

```text
source array slice = 19
source subresource = 19
destination subresource = 0
```

Source:

```text
3840×2176 NV12
```

Destination:

```text
3840×2176 NV12
```

Resultado:

```text
GPU copy submission = OK
```

No CPU intermediate.

---

# 16. NT SHARED HANDLE

`IDXGIResource1::CreateSharedHandle` funciona.

Ejemplo HANDLE:

```text
0x00000000000006D8
```

Access:

```text
DXGI_SHARED_RESOURCE_READ |
DXGI_SHARED_RESOURCE_WRITE
```

---

# 17. D3D12 OPEN SHARED HANDLE

D3D12 device creado en el mismo adapter físico D3D11.

Adapter:

```text
AMD Radeon (TM) RX 580
```

`OpenSharedHandle` → `ID3D12Resource`:

PASS

Properties:

```text
3840 × 2176
DXGI_FORMAT_NV12
D3D12_TEXTURE_LAYOUT_UNKNOWN
Flags = 0x20
```

---

# 18. SINCRONIZACIÓN

## Keyed mutex

Probado pero DESCARTADO como mecanismo cross-API.

D3D11:
`IDXGIKeyedMutex` sí existe.

D3D12:
`ID3D12Resource::QueryInterface<IDXGIKeyedMutex>` falla:

```text
HRESULT 0x80004002
E_NOINTERFACE
```

NO restaurar keyed mutex como arquitectura D3D11→D3D12.

## Shared GPU fence

Validado:

- ID3D11Device5 = YES
- ID3D11DeviceContext4 = YES
- ID3D11Fence creado
- D3D11_FENCE_FLAG_SHARED
- fence NT HANDLE creado
- abierto desde D3D12 como ID3D12Fence
- D3D11 `Signal(value=1)` = OK
- D3D12 DIRECT queue `Wait(value=1)` = OK

Cross-API GPU synchronization:

**VALID**

No CPU WaitForSingleObject como prueba.

Este era el STEP 32/40:

**PASS**

---

# 19. PASOS 33–40 REALIZADOS POR CURSOR

Cursor implementó:

```text
tests/gpu_d3d11_interop/src/wgpu_hal_interop.rs
tests/gpu_d3d11_interop/src/render_path.rs
tests/gpu_d3d11_interop/src/multi_frame.rs
tests/gpu_d3d11_interop/src/visual_validation.rs
tests/gpu_d3d11_interop/shaders/nv12_to_rgb.wgsl
```

Y actualizó:

```text
docs/gpu/GPU_EXPERIMENT_2.md
```

---

# 20. STEP 33 — WGPU-HAL

Mecanismo reportado:

```text
device.as_hal::<Dx12>()
→ OpenSharedHandle on NT texture handle
→ shared fence Wait
→ wgpu_hal::dx12::Device::texture_from_raw
→ create_texture_from_hal
```

Auditoría confirmó:

- recurso real llega a wgpu
- no synthetic replacement
- pointers imported vs wrapped coinciden
- no CPU upload replacement

## Adapter discovery

Cursor encontró inicialmente:

```text
Microsoft Basic Render Driver
```

en lugar de la RX580.

La implementación pasó a inicializar/seleccionar wgpu antes de FFmpeg/D3D11VA.

Después:

```text
AMD Radeon RX 580
```

IMPORTANTE:

La auditoría determinó que esto debe documentarse como:

**observación específica de Windows 10 + RX580/driver de este equipo**

NO como ley universal de DXGI.

---

# 21. STEP 34 — NV12 PLANES

Cursor reporta PASS.

Real imported NV12 resource:

- Plane0 → R8Unorm
- Plane1 → Rg8Unorm

No synthetic textures en el crate de Experiment 2.

---

# 22. STEP 35 — SHADER

Cursor reporta conexión de los planos reales al WGSL.

Shader:

```text
BT.709
limited range
```

Crop previsto:

```text
2160 / 2176
```

para no mostrar las 16 líneas de padding del decoder.

---

# 23. STEP 36 — PRIMER PROBLEMA DE VALIDACIÓN

Inicialmente Cursor declaró PASS porque hizo:

```text
queue.submit
present
```

Pero auditoría descubrió que:

- solo abrió ventana 256×256
- no hubo screenshot
- no hubo inspección humana
- no se comprobó green/purple/chroma
- no se comprobó contenido real visualmente

Por tanto:

**REAL FRAME VISUALLY VALIDATED = NO**

---

# 24. MULTI-FRAME / PERFORMANCE

Primera versión:

- reportaba 90 frames
- en realidad contador empezaba en 1 + 89 iterations
- fence `OpenSharedHandle` se hacía cada frame

Auditoría detectó ambos problemas.

Se corrigieron.

## Corrective validation

Resultado:

```text
Compilation: PASS
Architecture unchanged: YES
Production crates modified: NO
Cached D3D12 fence: YES
OpenSharedHandle fence calls during frame loop: 0
Real frames decoded: 90
GPU copies: 90
Frames rendered: 90
Present calls: 90
Wall-clock elapsed: 1.489 s
Corrected wall-clock FPS: 60.44
Fixture FPS: ~29.97
Throughput >= fixture rate: YES
```

No se usan:

```text
av_hwframe_transfer_data
swscale
CPU RGBA
GPU→CPU→GPU
synthetic substitution
```

Resource reuse:

```text
BOUNDED
```

Leak concern:

```text
NONE
```

Fence:
abierto una vez al inicio y cacheado.

---

# 25. INTERPRETACIÓN CORRECTA DEL 60.44 FPS

Es:

**wall-clock end-to-end throughput**

Incluye según el experimento:

- decode
- D3D11 GPU copy submission
- sync
- wgpu/D3D12 render submit
- present

NO es:

- GPU timestamp
- GPU execution time puro

No llamarlo GPU execution FPS.

Sí demuestra que el pipeline medido procesa por encima de ~29.97 FPS en esta máquina, siempre sujeto a que el render sea visualmente correcto.

---

# 26. VISUAL VALIDATION MODE

Se añadió:

```powershell
cargo run -p gpu-d3d11-interop -- --visual
```

Archivo:

```text
visual_validation.rs
```

Características reportadas:

- winit event loop
- wgpu pre-init
- FFmpeg real
- real HEVC frames
- full GPU-resident path
- loop/restart fixture on EOF
- av_seek_frame + avcodec_flush_buffers
- ESC / close window exits
- resize handling
- 1280×720
- stays open until user closes

---

# 27. ESTADO CRÍTICO ACTUAL — VISUAL FAILURE

El usuario ejecutó:

```powershell
cd C:\proyectos-cursor\do-vale-studio-4
cargo run -p gpu-d3d11-interop -- --visual
```

La ventana SÍ apareció y se quedó abierta.

**Pero la salida fue una pantalla COMPLETAMENTE VERDE.**

No apareció vídeo, imagen ni contenido reconocible.

Por tanto:

```text
HUMAN VISUAL VALIDATION = FAIL
```

Y:

```text
GPU EXPERIMENT 2 FINAL STATUS = PARTIAL
```

NO marcar PASS.

---

# 28. QUÉ SIGNIFICA EL VERDE

La arquitectura hasta wgpu está respaldada por evidencia de código/auditoría:

```text
HEVC
→ D3D11VA
→ AV_PIX_FMT_D3D11
→ real ID3D11Texture2D
→ GPU copy
→ shareable NV12
→ NT HANDLE
→ ID3D12Resource
→ shared GPU fence
→ wgpu-hal
```

Pero el resultado verde demuestra que **la etapa de uso/render de los planos NV12 no está visualmente correcta todavía**.

Posibles áreas a investigar, SIN asumir cuál es la causa:

1. D3D12 resource state / resource barriers
2. wgpu-hal external resource ownership/state assumptions
3. NV12 plane SRV/view creation
4. PlaneSlice mapping
5. plane formats
6. shader bindings
7. UV plane coordinates
8. texture view dimension
9. synchronization ordering
10. resource state expected by wgpu
11. whether D3D11 GPU copy is fully visible to D3D12/wgpu at sampling time
12. whether wgpu imported resource uses correct usage flags
13. `texture_from_raw` invariants
14. crop/UV coordinate calculation
15. render pipeline / bind groups
16. surface/window path
17. shader conversion mathematics
18. actual sampled values being zero/invalid
19. padded texture dimensions / plane dimensions
20. D3D12 plane-specific SRV descriptors

No fallback CPU permitido.

---

# 29. MUY IMPORTANTE PARA EL NUEVO CHAT

No decir:

```text
Experiment 2 PASS
```

Estado exacto:

```text
40 / 40 attempted
Architecture integrity: PASS
Technical interop chain: largely PASS
Human visual validation: FAIL
Final Experiment 2 status: PARTIAL
Current blocker: solid green output in --visual mode
```

---

# 30. PRÓXIMO OBJETIVO

NO rehacer los pasos 1–32.

NO desmontar la arquitectura.

NO comenzar integración en production crates.

Primero:

**diagnosticar por qué el recurso NV12 real importado en wgpu produce una pantalla verde.**

Trabajar como una investigación controlada.

Idealmente crear un plan de diagnóstico incremental.

Cada prueba debe aislar una hipótesis.

Ejemplos de pruebas útiles:

- validar wgpu surface rendering con color/patrón conocido
- validar shader pipeline con synthetic NV12 dentro del MISMO visual path
- luego sustituir synthetic por real external NV12
- comprobar plane0 solo como grayscale
- comprobar plane1 separadamente
- validar SRV PlaneSlice 0/1
- verificar D3D12 resource state antes de sampling
- comprobar barriers/state transitions
- inspeccionar wgpu-hal `texture_from_raw` requirements
- comprobar usage flags y subresource state
- comprobar synchronization/fence values

IMPORTANTE:

Las texturas sintéticas pueden usarse como **diagnóstico aislado** para determinar si falla el shader/surface, PERO no pueden usarse para declarar que el recurso real funciona.

Si synthetic funciona y real no:
problema está en external-resource/planes/state/sync.

Si synthetic también sale verde:
problema está en shader/render/surface/bindings.

---

# 31. REGLA DE TRABAJO CON CURSOR

El usuario prefiere no ir comando por comando.

Usar prompts de tareas completas para Cursor.

Cursor puede:

- corregir errores normales de compilación
- ejecutar pruebas
- avanzar entre pruebas diagnósticas

Pero si encuentra una limitación arquitectónica real:

**DEBE PARAR**

No:

- inventar fallback
- meter CPU readback
- cambiar a software decode
- cambiar arquitectura silenciosamente

El usuario traerá el error al chat para decidir qué hacer.

---

# 32. ESTILO DE ASISTENCIA

Responder en español.

Ser claro y técnico.

No dar veinte comandos manuales si Cursor puede hacer la tarea.

Cuando haya progreso, indicar el punto actual.

Para este momento usar:

```text
Experiment 2: 40/40 attempted — PARTIAL
Current blocker: visual render = solid green
```

No llamar PASS hasta que se vea vídeo real correctamente.

---

# 33. PRIMERA TAREA RECOMENDADA EN EL NUEVO CHAT

Preparar un prompt para Cursor para hacer un:

**GREEN FRAME ROOT-CAUSE DIAGNOSTIC**

sin cambiar arquitectura.

Debe:

1. auditar `render_path.rs`
2. auditar `wgpu_hal_interop.rs`
3. auditar `visual_validation.rs`
4. auditar `nv12_to_rgb.wgsl`
5. comprobar resource states
6. comprobar NV12 plane SRVs
7. comprobar wgpu-hal import invariants
8. comparar el path visual actual con Experiment 1
9. hacer pruebas diagnósticas ordenadas
10. parar cuando identifique la primera causa real
11. no aplicar fallback arquitectónico sin aprobación

---

# 34. RESUMEN ULTRACORTO

Studio 4 ha demostrado con éxito:

```text
Real 4K HEVC
→ FFmpeg D3D11VA
→ AV_PIX_FMT_D3D11
→ ID3D11Texture2D NV12
→ GPU-only CopySubresourceRegion
→ shareable D3D11 NV12
→ NT HANDLE
→ ID3D12Resource
→ shared ID3D11Fence / ID3D12Fence
→ wgpu-hal imported resource
```

90-frame technical run:

```text
90 decoded
90 GPU copies
90 renders
90 presents
1.489 s
60.44 wall-clock FPS
```

Sin:

```text
av_hwframe_transfer_data
swscale
CPU RGBA
GPU→CPU→GPU
```

PERO:

```text
--visual mode shows SOLID GREEN.
```

Por tanto:

```text
FINAL STATUS: PARTIAL
NEXT: diagnose NV12/wgpu/render green output.
```
