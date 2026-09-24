<?php
            function f(): void {
                try {
                    doSomething();
                } catch (Exception $e) {
                    throw $e;
                }
            }
            function doSomething(): void {}
        