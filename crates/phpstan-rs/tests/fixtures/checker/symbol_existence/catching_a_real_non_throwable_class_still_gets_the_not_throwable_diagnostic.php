<?php
            class NotAnException {}
            function f() {
                try {
                    doSomething();
                } catch (NotAnException $e) {
                }
            }
            function doSomething() {}
        