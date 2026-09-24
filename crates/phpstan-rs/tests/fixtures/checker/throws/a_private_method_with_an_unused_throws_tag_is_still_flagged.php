<?php
            class Repo {
                /** @throws \RuntimeException */
                private function find(): void {
                    echo 'never actually throws';
                }
            }
        