<?php
            namespace App\Other;
            interface ResponseInterface {}

            namespace App\Impl;
            use App\Other\ResponseInterface;
            class RealResponse implements ResponseInterface {}

            namespace App;
            use App\Other\ResponseInterface;
            use App\Impl\RealResponse;
            function make() {
                return new class {
                    public function handle(): ResponseInterface {
                        return new RealResponse();
                    }
                };
            }
        