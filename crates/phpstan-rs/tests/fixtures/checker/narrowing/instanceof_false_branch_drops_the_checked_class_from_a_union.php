<?php
            class Mysqli {}
            class PgConnection {}
            function needsPgConnection(PgConnection $c): void {}
            function dbEscape(Mysqli|PgConnection $db): void {
                if ($db instanceof Mysqli) {
                    return;
                }
                needsPgConnection($db);
            }
        