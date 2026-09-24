<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType(): self { return $this; }
        }
        namespace App\Rules;
        class SomeRule {
            public function processNode(\PHPStan\FakeNs\Widget $scope) {
                $scope->getType()->getType();
            }
        }
        