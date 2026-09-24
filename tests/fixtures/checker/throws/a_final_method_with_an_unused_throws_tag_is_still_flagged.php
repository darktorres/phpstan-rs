<?php
            class Repo {
                /** @throws \RuntimeException */
                final public function find(): void {
                    echo 'never actually throws';
                }
            }
        