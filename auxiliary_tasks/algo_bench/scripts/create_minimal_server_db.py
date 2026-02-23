#!/usr/bin/env python3
"""Create minimal server.sqlite with decks used for algo_bench eval."""

import sqlite3
import json
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATA_DIR = SCRIPT_DIR.parent / "data"
OUTPUT_DB = DATA_DIR / "server.sqlite"

DECKS = [
    {
        "deck_id": "51cd0af4-1a79-4542-a342-fd1a054d614e",
        "user_id": "a61a0030-ce16-4de5-baa4-ba145adcf7ac",
        "name": "Overzealous-v0",
        "cards_json": '[{"card_def_id":"CG-6","count":4},{"card_def_id":"CG-55","count":4},{"card_def_id":"CG-37","count":4},{"card_def_id":"DF-93","count":4},{"card_def_id":"DF-33","count":4},{"card_def_id":"DF-61","count":4},{"card_def_id":"DF-3","count":4},{"card_def_id":"CG-73","count":4},{"card_def_id":"DF-79","count":4},{"card_def_id":"DF-75","count":4},{"card_def_id":"ENERGY-FIRE","count":20}]',
        "created_at": 1768109155,
        "updated_at": 1768109185,
        "is_public": 1,
    },
    {
        "deck_id": "fdb6aecb-7efd-4ec1-9d2d-9138a3096c99",
        "user_id": "a61a0030-ce16-4de5-baa4-ba145adcf7ac",
        "name": "Overzealous-v1",
        "cards_json": '[{"card_def_id":"CG-6","count":4},{"card_def_id":"CG-55","count":4},{"card_def_id":"CG-37","count":4},{"card_def_id":"DF-93","count":4},{"card_def_id":"DF-33","count":4},{"card_def_id":"DF-61","count":4},{"card_def_id":"CG-73","count":4},{"card_def_id":"DF-79","count":4},{"card_def_id":"DF-75","count":4},{"card_def_id":"ENERGY-FIRE","count":20},{"card_def_id":"DF-95","count":4}]',
        "created_at": 1768115274,
        "updated_at": 1768115310,
        "is_public": 1,
    },
    {
        "deck_id": "9f2f4d6a-a9e1-4aa0-ac22-205837cc95a8",
        "user_id": "a61a0030-ce16-4de5-baa4-ba145adcf7ac",
        "name": "Sceptile ex Delta",
        "cards_json": '[{"card_def_id":"CG-68","count":4},{"card_def_id":"CG-19","count":4},{"card_def_id":"CG-96","count":4},{"card_def_id":"DF-61","count":4},{"card_def_id":"DF-33","count":4},{"card_def_id":"CG-73","count":4},{"card_def_id":"CG-71","count":4},{"card_def_id":"CG-84","count":3},{"card_def_id":"DF-80","count":2},{"card_def_id":"DF-75","count":2},{"card_def_id":"DF-79","count":2},{"card_def_id":"ENERGY-PSYCHIC","count":10},{"card_def_id":"DF-93","count":4},{"card_def_id":"ENERGY-FIRE","count":9}]',
        "created_at": 1768106751,
        "updated_at": 1768108340,
        "is_public": 1,
    },
    {
        "deck_id": "1863a389-10f7-48e9-a04a-e73783b02682",
        "user_id": "a61a0030-ce16-4de5-baa4-ba145adcf7ac",
        "name": "Sceptile ex Lock Control",
        "cards_json": '[{"card_def_id":"CG-67","count":4},{"card_def_id":"CG-32","count":4},{"card_def_id":"CG-96","count":4},{"card_def_id":"CG-94","count":2},{"card_def_id":"DF-75","count":4},{"card_def_id":"DF-79","count":4},{"card_def_id":"CG-73","count":3},{"card_def_id":"DF-73","count":3},{"card_def_id":"DF-82","count":2},{"card_def_id":"CG-72","count":2},{"card_def_id":"CG-83","count":3},{"card_def_id":"CG-78","count":3},{"card_def_id":"CG-75","count":2},{"card_def_id":"CG-74","count":2},{"card_def_id":"CG-85","count":1},{"card_def_id":"DF-83","count":1},{"card_def_id":"ENERGY-PSYCHIC","count":16}]',
        "created_at": 1768114447,
        "updated_at": 1768114447,
        "is_public": 1,
    },
    {
        "deck_id": "green-cyclone-theme-001",
        "user_id": "a61a0030-ce16-4de5-baa4-ba145adcf7ac",
        "name": "Green Cyclone",
        "cards_json": '[{"card_def_id":"CG-28","count":1},{"card_def_id":"CG-23","count":2},{"card_def_id":"CG-32","count":2},{"card_def_id":"CG-33","count":1},{"card_def_id":"CG-34","count":2},{"card_def_id":"CG-45","count":4},{"card_def_id":"CG-53","count":2},{"card_def_id":"CG-60","count":4},{"card_def_id":"CG-67","count":4},{"card_def_id":"CG-69","count":4},{"card_def_id":"LM-73","count":2},{"card_def_id":"CG-73","count":2},{"card_def_id":"CG-78","count":2},{"card_def_id":"CG-86","count":2},{"card_def_id":"CG-87","count":2},{"card_def_id":"ENERGY-GRASS","count":24}]',
        "created_at": 1768114447,
        "updated_at": 1768114447,
        "is_public": 1,
    },
]

USERS = [
    ("a61a0030-ce16-4de5-baa4-ba145adcf7ac", "algo_bench", 1768100000),
]


def main():
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    if OUTPUT_DB.exists():
        OUTPUT_DB.unlink()

    conn = sqlite3.connect(OUTPUT_DB)
    conn.executescript("""
        CREATE TABLE users (
            user_id TEXT PRIMARY KEY,
            username TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE decks (
            deck_id TEXT PRIMARY KEY,
            user_id TEXT REFERENCES users(user_id),
            name TEXT NOT NULL,
            cards_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            is_public INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX idx_decks_user ON decks(user_id);
    """)

    for user_id, username, created_at in USERS:
        conn.execute(
            "INSERT INTO users (user_id, username, created_at) VALUES (?, ?, ?)",
            (user_id, username, created_at),
        )

    for d in DECKS:
        conn.execute(
            """INSERT INTO decks (deck_id, user_id, name, cards_json, created_at, updated_at, is_public)
               VALUES (?, ?, ?, ?, ?, ?, ?)""",
            (
                d["deck_id"],
                d["user_id"],
                d["name"],
                d["cards_json"],
                d["created_at"],
                d["updated_at"],
                d["is_public"],
            ),
        )

    conn.commit()
    conn.close()
    print(f"Created {OUTPUT_DB} with {len(DECKS)} decks")


if __name__ == "__main__":
    main()
