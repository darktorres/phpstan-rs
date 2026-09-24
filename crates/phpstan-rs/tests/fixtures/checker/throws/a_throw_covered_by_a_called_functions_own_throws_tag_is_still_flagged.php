<?php
            /** @throws \RuntimeException */
            function inner(): void {
                throw new \RuntimeException('inner is fully documented');
            }
            function outer(): void {
                throw new \RuntimeException('outer has no tag of its own');
            }
        