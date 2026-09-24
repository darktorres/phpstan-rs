<?php
            function f() {
                try {
                    doSomething();
                } catch (InvalidArgumentException $e) {
                }
            }
            function doSomething() {}
        