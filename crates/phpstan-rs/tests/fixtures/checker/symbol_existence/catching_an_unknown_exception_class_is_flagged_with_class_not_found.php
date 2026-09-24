<?php
            function f() {
                try {
                    doSomething();
                } catch (TotallyMadeUpException $e) {
                }
            }
            function doSomething() {}
        