# Deck Data for Algo Bench

This directory contains the deck data used for the algo_bench evaluation.

## Contents

- **server.sqlite** – Minimal SQLite DB with decks (Overzealous-v0, Overzealous-v1, Sceptile ex Delta, Sceptile ex Lock Control, Green Cyclone). Created by `scripts/create_minimal_server_db.py`.
- **cards.sqlite** – Card metadata (from overzealous). Required for `CardMetaMap` and game resolution.
- **decks/** – JSON deck definitions for reference (overzealous_v0.json, sceptile_ex_delta.json, green_cyclone.json).

## Regenerating server.sqlite

```bash
python3 scripts/create_minimal_server_db.py
```

## Source

Decks exported from the overzealous server DB. Green Cyclone is a theme deck from Bulbapedia.
