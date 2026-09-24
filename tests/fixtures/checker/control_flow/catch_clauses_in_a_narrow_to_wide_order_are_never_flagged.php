<?php
            function doStuff4(): void {}
            function test(): void {
                try {
                    doStuff4();
                } catch (\RuntimeException $e) {
                    echo "specific";
                } catch (\Exception $e) {
                    echo "general";
                }
            }
        