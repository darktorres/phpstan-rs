<?php
            function doStuff2(): void {}
            function test(): void {
                try {
                    doStuff2();
                } catch (\Exception $e) {
                    echo "outer";
                } catch (\RuntimeException $e) {
                    echo "inner";
                }
            }
        