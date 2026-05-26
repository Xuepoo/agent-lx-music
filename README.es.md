# agent-lx-music

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

Un reproductor de música CLI inspirado en la filosofía Unix, impulsado por Rust y compatible con los scripts de fuentes de lx-music. Elimina por completo el pesado framework Electron, ejecutando los scrapers JS dentro de un entorno QuickJS seguro y aislado (`rquickjs`) y delegando la decodificación y reproducción de audio de alta fidelidad a una instancia `mpv` sin cabeza (headless) mediante un demonio POSIX independiente (`setsid`).

---

## Características Principales

- **Buzón de Arena QuickJS Aislado**: Ejecuta scripts de fuentes tradicionales de `lx-music` de forma rápida y segura en un sandbox QuickJS optimizado con [rquickjs](https://github.com/DelSkayn/rquickjs).
- **Diseño de Demonio POSIX**: Utiliza el mecanismo `setsid` para lanzar `mpv` en un grupo de procesos independiente en segundo plano, permitiendo un control no bloqueante de la reproducción que sigue activo incluso al cerrar la terminal.
- **Caché Transparente en SQLite**: Almacena de forma local listas de reproducción, historial con autopurga según antigüedad y favoritos. Almacena de forma transparente la letra LRC para accesos instantáneos y sin red.
- **Gestión Estática de Letras LRC y Portadas**: Extracción a alta velocidad de letras LRC sincronizadas (con traducciones y transcripciones fonéticas). Detección de firmas mediante **Magic Bytes** para omitir cabeceras MIME inestables y autocompletar extensiones de imagen correctas.
- **Despliegue en Contenedores**: Totalmente compatible con Podman (rootless) y Docker, permitiendo el redireccionamiento directo del audio a través de los sockets PulseAudio/Pipewire del host.
- **Preparado para IA Multimodal**: Incluye habilidades IA compatibles con XDG (`music-discovery`, `audio-analysis`, `listening-companion`), permitiendo a modelos de lenguaje (LLM) como Gemini 1.5 Pro analizar, buscar y chatear sobre la música contigo.

---

## Instalación y Configuración

Para compilar desde el código fuente (requiere la cadena de herramientas de Rust):

```bash
# Clonar el repositorio
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# Compilar en modo release
cargo build --release

# Mostrar la ayuda global
./target/release/alx --help
```

---

## Referencia Rápida de Comandos

```bash
# 1. Registrar una fuente de música
alx source add ./my-sixyin-source.js

# 2. Buscar en todas las plataformas (devuelve CLI IDs cortos y dinámicos)
alx search "周杰伦 晴天"

# 3. Iniciar reproducción a través del demonio mpv en segundo plano
alx play <cli_id>

# 4. Controlar la reproducción de forma asíncrona
alx now                    # Muestra la tarjeta de progreso en tiempo real
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # Cierra por completo el demonio de mpv en segundo plano

# 5. Obtener letras y portadas
alx lyric <cli_id>         # Imprime la letra LRC sincronizada
alx lyric <cli_id> --save  # Exporta a un archivo .lrc en la carpeta de descargas
alx pic <cli_id> --save    # Descarga la portada con validación de extensión automática
```

---

## Documentación Técnica

Todos los detalles de diseño, API y modelos de datos están ubicados en la carpeta `docs`:
- [Especificación de Requisitos](docs/REQUIREMENTS.md) — Desglose completo de funcionalidades
- [Arquitectura Técnica](docs/ARCHITECTURE.md) — Diseño de desacoplamiento y mpv IPC
- [Referencia de la CLI](docs/CLI.md) — Documentación de comandos y opciones
- [API de Puente de Fuentes](docs/SOURCE-API.md) — Contrato de ejecución de QuickJS
- [Configuración de Rutas XDG](docs/CONFIG.md) — Resolución de variables de entorno
- [Esquema de Base de Datos SQLite](docs/DATA-MODEL.md) — Estructura de tablas y relaciones

---

## Licencia

Este proyecto está bajo la licencia MIT.
