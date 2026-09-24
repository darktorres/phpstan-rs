<?php
            class Banned extends \Exception {}
            function f(): void {
                try {
                } catch (Banned $e) {
                }
            }
        