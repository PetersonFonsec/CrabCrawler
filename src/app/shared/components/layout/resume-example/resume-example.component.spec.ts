import { ComponentFixture, TestBed } from '@angular/core/testing';

import { ResumeExampleComponent } from './resume-example.component';

describe('ResumeExampleComponent', () => {
  let component: ResumeExampleComponent;
  let fixture: ComponentFixture<ResumeExampleComponent>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [ResumeExampleComponent]
    })
    .compileComponents();

    fixture = TestBed.createComponent(ResumeExampleComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
