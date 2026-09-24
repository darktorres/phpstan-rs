<?php
            function noThrowsTagAtAll(): void {}
            /** @throws \RuntimeException */
            function f(): void {
                noThrowsTagAtAll();
                echo 'still never throws RuntimeException';
            }
        