<?php
            interface Shape {}
            if (!class_exists('Polyfilled')) {
                class Polyfilled extends Shape {
                    public callable $cb;
                }
            }
        