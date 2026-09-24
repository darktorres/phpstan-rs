<?php
            /** @throws \RuntimeException */
            function inner(): void {
                throw new \RuntimeException('real');
            }
            /** @throws \RuntimeException */
            function outer(): void {
                inner();
            }
        