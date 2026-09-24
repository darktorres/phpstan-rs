<?php
            class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws, and nothing overrides this yet';
                }
            }
        