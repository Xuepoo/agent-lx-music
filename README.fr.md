# agent-lx-music

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Español](README.es.md)

Un lecteur de musique CLI inspiré de la philosophie Unix, propulsé par Rust et compatible avec les scripts de sources lx-music. Il abandonne complètement le framework lourd Electron, exécutant les scripts JS dans un bac à sable QuickJS hautement optimisé (`rquickjs`) et déléguant le décodage et la lecture audio haute fidélité à une instance `mpv` headless via un démon POSIX détaché (`setsid`).

---

## Fonctionnalités Clés

- **Bac à Sable QuickJS Isolé** : Exécute les scripts de sources `lx-music` existants de manière sécurisée et rapide grâce à l'intégration de [rquickjs](https://github.com/DelSkayn/rquickjs).
- **Architecture Démon POSIX** : Lance `mpv` dans un groupe de processus détaché avec `setsid`, permettant de contrôler la lecture de manière asynchrone sans bloquer votre terminal ou s'arrêter à la fermeture de la session.
- **Cache transparent SQLite** : Enregistre localement les listes de lecture, l'historique d'écoute (avec purge automatique) et les favoris. Met en cache de manière transparente les paroles LRC pour un accès instantané et sans réseau.
- **Gestion des Paroles LRC et Pochettes** : Extraction ultra-rapide des paroles LRC (avec traductions et transcriptions phonétiques) et détection des extensions d'images par **Magic Bytes** pour contourner les en-têtes MIME instables.
- **Conteneurisation Directe** : Totalement compatible avec Podman (rootless) et Docker, supportant le pass-through audio PulseAudio/Pipewire vers l'hôte.
- **Prêt pour les Agents IA** : Intègre des fichiers de compétences IA conformes XDG (`music-discovery`, `audio-analysis`, `listening-companion`), permettant à des LLM multimodaux (comme Gemini 1.5 Pro) d'analyser, rechercher et discuter de musique en temps réel.

---

## Installation

Pour compiler à partir des sources (nécessite la chaîne d'outils Rust) :

```bash
# Cloner le dépôt
git clone https://github.com/Xuepoo/agent-lx-music.git
cd agent-lx-music

# Compiler la version release
cargo build --release

# Afficher l'aide générale
./target/release/alx --help
```

---

## Guide Rapide de Commandes

```bash
# 1. Enregistrer une source de musique
alx source add ./my-sixyin-source.js

# 2. Rechercher sur toutes les plateformes (renvoie des CLI IDs courts)
alx search "周杰伦 晴天"

# 3. Lancer la lecture audio via le démon mpv détaché
alx play <cli_id>

# 4. Contrôler la lecture de manière asynchrone
alx now                    # Affiche la carte de progression en temps réel
alx volume +10 / alx volume -10
alx seek +30 / alx seek 2:30
alx pause / alx resume / alx stop
alx quit                   # Arrête proprement le démon mpv en arrière-plan

# 5. Récupérer les paroles et pochettes
alx lyric <cli_id>         # Affiche les paroles LRC synchronisées
alx lyric <cli_id> --save  # Exporte dans un fichier .lrc dans le dossier de téléchargement
alx pic <cli_id> --save    # Télécharge la pochette avec correction automatique d'extension
```

---

## Documentation Technique

Toutes les spécifications, protocoles et modèles de données se trouvent dans le dossier `docs` :
- [Requirements](docs/REQUIREMENTS.md) — Spécifications complètes des fonctionnalités
- [Architecture](docs/ARCHITECTURE.md) — Conception modulaire et communication mpv IPC
- [CLI Reference](docs/CLI.md) — Documentation de toutes les commandes et options
- [Source API Bridge](docs/SOURCE-API.md) — Contrat d'exécution QuickJS
- [XDG Path Config](docs/CONFIG.md) — Résolution des variables d'environnement
- [SQLite Data Schema](docs/DATA-MODEL.md) — Modèle relationnel de la base de données

---

## Licence

Ce projet est sous licence MIT.
