<?php
            function doStuff5(): void {}
            function test(): void {
                try {
                    doStuff5();
                } catch (\LogicException $e) {
                    echo "logic";
                } catch (\RuntimeException $e) {
                    echo "runtime";
                }
            }
        