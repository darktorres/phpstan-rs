<?php
            function doStuff3(): void {}
            function test(): void {
                try {
                    doStuff3();
                } catch (\RuntimeException $e) {
                    echo "first";
                } catch (\RuntimeException $e) {
                    echo "second";
                }
            }
        