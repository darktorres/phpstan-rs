<?php
            interface Finder {
                /** @throws \RuntimeException */
                public function find(): void;
            }
            class Repo implements Finder {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws, and this overrides Finder::find()';
                }
            }
        