import { FileText, File, TestTube2 } from 'lucide-react';

interface ArtifactIconProps {
  artifactType: string;
  className?: string;
}

export function ArtifactIcon({ artifactType, className }: ArtifactIconProps) {
  switch (artifactType) {
    case 'description':
    case 'analysis':
      return <FileText className={className} />;
    case 'test_results':
    case 'test':
    case 'coverage':
      return <TestTube2 className={className} />;
    default:
      return <File className={className} />;
  }
}
