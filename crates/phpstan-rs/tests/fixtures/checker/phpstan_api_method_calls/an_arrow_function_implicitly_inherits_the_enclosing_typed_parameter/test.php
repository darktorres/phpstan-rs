<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace App;
        function f(\PHPStan\FakeNs\Widget $scope) {
            $cb = fn() => $scope->getType();
        }
        