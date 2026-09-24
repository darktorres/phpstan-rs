<?php
            final class Repo {
                /** @throws \RuntimeException */
                public function find(): void {
                    echo 'never actually throws';
                }
            }
        