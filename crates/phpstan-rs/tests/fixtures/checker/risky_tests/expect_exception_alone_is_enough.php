<?php
        namespace PHPUnit\Framework {
            class TestCase {}
        }
        namespace {
            
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $this->expectException(\Exception::class);
                    throw new \Exception();
                }
            }
        
        }
        